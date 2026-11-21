//! X11: 画面は RandR のモニター、取り込みは MIT-SHM（XShmGetImage）、更新の検知は XDamage、カーソルは XFixes、
//! 入力は XTest で行う。

use std::{
    os::fd::AsFd,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{
        Event,
        damage::{self, ConnectionExt as _},
        randr::ConnectionExt as _,
        shm::{self, ConnectionExt as _},
        xfixes::{self, ConnectionExt as _},
        xproto::{self, AtomEnum, ConnectionExt as _, ImageFormat},
        xtest::ConnectionExt as _,
    },
    rust_connection::RustConnection,
};

use super::{
    Button, Cursor, Error, Frame, ImportFormats, Inject, PixelFormat, Pointer, Session, Shm,
    Source, Touch, Wake,
};
use crate::SourceKind;

impl From<x11rb::errors::ConnectError> for Error {
    fn from(e: x11rb::errors::ConnectError) -> Self {
        Self(format!("X11: {e}"))
    }
}
impl From<x11rb::errors::ConnectionError> for Error {
    fn from(e: x11rb::errors::ConnectionError) -> Self {
        Self(format!("X11: {e}"))
    }
}
impl From<x11rb::errors::ReplyError> for Error {
    fn from(e: x11rb::errors::ReplyError) -> Self {
        Self(format!("X11: {e}"))
    }
}
impl From<x11rb::errors::ReplyOrIdError> for Error {
    fn from(e: x11rb::errors::ReplyOrIdError) -> Self {
        Self(format!("X11: {e}"))
    }
}

/// 取り込み先の共有メモリの数。表示中・転送中・書き込み中の 3 枚で足りる。
const SHM_COUNT: usize = 3;

/// モニター（取り込み元）。`target` は RandR のモニター名のアトム（RandR が無ければ 1 で画面全体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Monitor {
    target: u32,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

fn monitors(
    conn: &RustConnection,
    root: xproto::Window,
) -> Result<Vec<(Monitor, String, Option<String>)>, Error> {
    let randr = conn
        .extension_information(x11rb::protocol::randr::X11_EXTENSION_NAME)?
        .is_some()
        && conn
            .randr_query_version(1, 5)?
            .reply()
            .is_ok_and(|v| (v.major_version, v.minor_version) >= (1, 5));
    if !randr {
        let screen = &conn.setup().roots[0];
        let whole = Monitor {
            target: 1,
            x: 0,
            y: 0,
            width: screen.width_in_pixels,
            height: screen.height_in_pixels,
        };
        return Ok(vec![(whole, "Screen".into(), None)]);
    }
    let edid = conn.intern_atom(true, b"EDID")?.reply()?.atom;
    let mut found = Vec::new();
    for m in conn.randr_get_monitors(root, true)?.reply()?.monitors {
        let name = String::from_utf8_lossy(&conn.get_atom_name(m.name)?.reply()?.name).into_owned();
        let product = m.outputs.first().and_then(|&output| {
            let data = conn
                .randr_get_output_property(output, edid, AtomEnum::ANY, 0, 64, false, false)
                .ok()?
                .reply()
                .ok()?
                .data;
            super::edid_name(&data)
        });
        let monitor = Monitor {
            target: m.name,
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
        };
        found.push((monitor, name, product));
    }
    Ok(found)
}

pub(super) fn sources() -> Result<Vec<Source>, Error> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let found = monitors(&conn, root)?;
    let names = super::label(
        &found
            .iter()
            .map(|(_, n, p)| (n.clone(), p.clone()))
            .collect::<Vec<_>>(),
    );
    Ok(found
        .into_iter()
        .zip(names)
        .map(|((m, ..), name)| Source {
            name,
            kind: SourceKind::Screen,
            icon: None,
            app: None,
            target: m.target as isize,
        })
        .collect())
}

pub(super) struct Capture {
    conn: Arc<RustConnection>,
    root: xproto::Window,
    monitor: Monitor,
    /// 画面の変化があったか（XDamage）。XDamage が無ければ常に取り込む。
    dirty: Arc<AtomicBool>,
    damage: Option<damage::Damage>,
    /// カーソルが変わったか（XFixes）。
    cursor_changed: Arc<AtomicBool>,
    cursor: Option<(u32, Option<Arc<Cursor>>)>,
    ring: Vec<(Arc<Shm>, shm::Seg)>,
    import: Option<ImportFormats>,
    pointer: Pointer,
    xtest: bool,
    stop: Arc<AtomicBool>,
    events: Option<thread::JoinHandle<()>>,
    /// 最後にモニターの位置・大きさを読んだ時刻（解像度や配置の変更に追従する）。
    checked: Instant,
}

impl Capture {
    pub fn new(source: Source, wake: Wake, import: Option<ImportFormats>) -> Result<Self, Error> {
        let (conn, screen) = x11rb::connect(None)?;
        let conn = Arc::new(conn);
        let root_screen = &conn.setup().roots[screen];
        let root = root_screen.root;
        // ZPixmap 32bpp・赤が上位の 24/32 ビット深度（B, G, R, X の並び）のみ扱う。
        let visual = root_screen
            .allowed_depths
            .iter()
            .flat_map(|d| &d.visuals)
            .find(|v| v.visual_id == root_screen.root_visual);
        let bpp = conn
            .setup()
            .pixmap_formats
            .iter()
            .find(|f| f.depth == root_screen.root_depth)
            .map(|f| f.bits_per_pixel);
        if bpp != Some(32) || visual.is_none_or(|v| v.red_mask != 0xff_0000) {
            return Err(Error(crate::failure::UNSUPPORTED_FORMAT.into()));
        }
        if conn
            .extension_information(shm::X11_EXTENSION_NAME)?
            .is_none()
            || conn
                .shm_query_version()?
                .reply()
                .is_ok_and(|v| (v.major_version, v.minor_version) < (1, 2))
        {
            return Err(Error("X11: MIT-SHM 1.2 is unavailable".into()));
        }
        let monitor = monitors(&conn, root)?
            .into_iter()
            .map(|(m, ..)| m)
            .find(|m| m.target as isize == source.target)
            .ok_or_else(|| Error(crate::failure::NO_PIXELS.into()))?;
        let damage = if conn
            .extension_information(damage::X11_EXTENSION_NAME)?
            .is_some()
            && conn.damage_query_version(1, 1)?.reply().is_ok()
        {
            let id = conn.generate_id()?;
            conn.damage_create(id, root, damage::ReportLevel::NON_EMPTY)?
                .check()?;
            Some(id)
        } else {
            None
        };
        let xfixes = conn
            .extension_information(xfixes::X11_EXTENSION_NAME)?
            .is_some()
            && conn.xfixes_query_version(5, 0)?.reply().is_ok();
        if xfixes {
            conn.xfixes_select_cursor_input(root, xfixes::CursorNotifyMask::DISPLAY_CURSOR)?
                .check()?;
        }
        let xtest = conn
            .extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)?
            .is_some()
            && conn.xtest_get_version(2, 2)?.reply().is_ok();
        let dirty = Arc::new(AtomicBool::new(true));
        let cursor_changed = Arc::new(AtomicBool::new(xfixes));
        let stop = Arc::new(AtomicBool::new(false));
        let events = {
            let (conn, dirty, changed, stop) = (
                conn.clone(),
                dirty.clone(),
                cursor_changed.clone(),
                stop.clone(),
            );
            thread::Builder::new()
                .name("desktop-x11-events".into())
                .spawn(move || watch(&conn, &dirty, &changed, &stop, &wake))
                .map_err(|e| Error(e.to_string()))?
        };
        Ok(Self {
            conn,
            root,
            monitor,
            dirty,
            damage,
            cursor_changed,
            cursor: None,
            ring: Vec::new(),
            import,
            pointer: Pointer::default(),
            xtest,
            stop,
            events: Some(events),
            checked: Instant::now(),
        })
    }

    /// 使っていない共有メモリ（無ければ `None`）。大きさが変われば X サーバーから切り離して作り直す。
    fn take(&mut self, size: usize) -> Result<Option<(Arc<Shm>, shm::Seg)>, Error> {
        if self.ring.first().is_some_and(|(s, _)| s.len < size) {
            for (_, seg) in self.ring.drain(..) {
                self.conn.shm_detach(seg)?;
            }
        }
        if let Some(found) = self.ring.iter().find(|(s, _)| s.free()) {
            return Ok(Some(found.clone()));
        }
        if self.ring.len() >= SHM_COUNT {
            return Ok(None);
        }
        let shm = Shm::new(size, self.import.as_ref())?;
        let seg = self.conn.generate_id()?;
        let fd = shm
            .fd()
            .try_clone_to_owned()
            .map_err(|e| Error(e.to_string()))?;
        self.conn.shm_attach_fd(seg, fd, false)?.check()?;
        self.ring.push((shm.clone(), seg));
        Ok(Some((shm, seg)))
    }

    fn inject(&mut self) -> Result<XTest<'_>, Error> {
        if !self.xtest {
            return Err(Error("X11: XTEST is unavailable".into()));
        }
        Ok(XTest {
            conn: &self.conn,
            root: self.root,
            monitor: self.monitor,
        })
    }
}

/// X サーバーのイベントを待ち、画面の変化とカーソルの変化を記録してワーカーを起こす。
fn watch(
    conn: &RustConnection,
    dirty: &AtomicBool,
    cursor: &AtomicBool,
    stop: &AtomicBool,
    wake: &Wake,
) {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: 50_000_000,
    };
    while !stop.load(Ordering::Acquire) {
        loop {
            match conn.poll_for_event() {
                Ok(Some(Event::DamageNotify(_))) => {
                    if !dirty.swap(true, Ordering::AcqRel) {
                        wake.wake();
                    }
                }
                Ok(Some(Event::XfixesCursorNotify(_))) => cursor.store(true, Ordering::Release),
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => return,
            }
        }
        let fd = conn.stream().as_fd();
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        let _ = poll(&mut fds, Some(&timeout));
    }
}

impl Session for Capture {
    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        if self.damage.is_some() && !self.dirty.load(Ordering::Acquire) {
            return Ok(None);
        }
        let Monitor { width, height, .. } = self.monitor;
        let stride = width as u32 * 4;
        let Some((shm, seg)) = self.take(stride as usize * height as usize)? else {
            return Ok(None);
        };
        // 取り込む前に下ろし、取り込み中の変化で次の通知が来るようにする。
        if let Some(damage) = self.damage {
            self.dirty.store(false, Ordering::Release);
            self.conn
                .damage_subtract(damage, x11rb::NONE, x11rb::NONE)?;
        }
        let image = self.conn.shm_get_image(
            self.root,
            self.monitor.x,
            self.monitor.y,
            width,
            height,
            !0,
            ImageFormat::Z_PIXMAP.into(),
            seg,
            0,
        )?;
        if let Err(error) = image.reply() {
            // 解像度の変更中などで範囲が画面からはみ出した。次の確認で位置と大きさを読み直す。
            self.checked = Instant::now() - Duration::from_secs(10);
            self.dirty.store(true, Ordering::Release);
            logger::debug!("X11 screen read failed", error = error);
            return Ok(None);
        }
        let import = self.import.as_ref();
        shm.frame(
            width as u32,
            height as u32,
            stride,
            PixelFormat::Bgrx,
            false,
            import,
        )
        .map(Some)
    }

    fn heartbeat(&mut self) -> Result<(), Error> {
        if self.checked.elapsed() < Duration::from_secs(1) {
            return Ok(());
        }
        self.checked = Instant::now();
        let Some(now) = monitors(&self.conn, self.root)?
            .into_iter()
            .map(|(m, ..)| m)
            .find(|m| m.target == self.monitor.target)
        else {
            return Err(Error(crate::failure::NO_PIXELS.into()));
        };
        if now != self.monitor {
            self.monitor = now;
            self.dirty.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn cursor(&mut self) -> Option<Arc<Cursor>> {
        if self.cursor_changed.swap(false, Ordering::AcqRel) {
            match self.conn.xfixes_get_cursor_image().ok()?.reply() {
                Ok(c)
                    if self
                        .cursor
                        .as_ref()
                        .is_some_and(|(s, _)| *s == c.cursor_serial) => {}
                Ok(c) => {
                    let image = (c.width > 0 && c.height > 0 && c.width <= 256 && c.height <= 256)
                        .then(|| {
                            let icon = super::unpremultiply_argb(
                                c.width.into(),
                                c.height.into(),
                                c.cursor_image.iter().copied(),
                            );
                            Arc::new(Cursor {
                                image: icon,
                                hotspot: (c.xhot.into(), c.yhot.into()),
                            })
                        });
                    self.cursor = Some((c.cursor_serial, image));
                }
                Err(_) => self.cursor = None,
            }
        }
        self.cursor.as_ref().and_then(|(_, c)| c.clone())
    }

    fn touch(&mut self, touch: Touch) -> Result<(), Error> {
        self.button(touch, Button::Left)
    }

    fn button(&mut self, touch: Touch, button: Button) -> Result<(), Error> {
        let mut pointer = std::mem::take(&mut self.pointer);
        let result = pointer.send(&mut self.inject()?, touch, button);
        self.pointer = pointer;
        result
    }

    fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), Error> {
        let mut pointer = std::mem::take(&mut self.pointer);
        let result = pointer.scroll(&mut self.inject()?, x, y, dx, dy);
        self.pointer = pointer;
        result
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let mut pointer = std::mem::take(&mut self.pointer);
        if let Ok(mut out) = self.inject() {
            pointer.release(&mut out);
        }
        self.stop.store(true, Ordering::Release);
        if let Some(events) = self.events.take() {
            let _ = events.join();
        }
        for (_, seg) in self.ring.drain(..) {
            let _ = self.conn.shm_detach(seg);
        }
        if let Some(damage) = self.damage {
            let _ = self.conn.damage_destroy(damage);
        }
        let _ = self.conn.flush();
    }
}

/// XTest で X サーバーへ直接ポインタの操作を送る。
struct XTest<'a> {
    conn: &'a RustConnection,
    root: xproto::Window,
    monitor: Monitor,
}

impl XTest<'_> {
    fn send(&self, kind: u8, detail: u8, x: i16, y: i16) -> Result<(), Error> {
        self.conn
            .xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.root, x, y, 0)?;
        Ok(())
    }

    fn click(&self, detail: u8, down: bool) -> Result<(), Error> {
        let kind = if down {
            xproto::BUTTON_PRESS_EVENT
        } else {
            xproto::BUTTON_RELEASE_EVENT
        };
        self.send(kind, detail, 0, 0)
    }
}

impl Inject for XTest<'_> {
    fn motion(&mut self, x: f32, y: f32) -> Result<(), Error> {
        let m = self.monitor;
        let px = m.x as i32 + super::pixel(x, m.width.into());
        let py = m.y as i32 + super::pixel(y, m.height.into());
        self.send(xproto::MOTION_NOTIFY_EVENT, 0, px as i16, py as i16)?;
        self.conn.flush()?;
        Ok(())
    }

    fn press(&mut self, button: Button, down: bool) -> Result<(), Error> {
        let detail = match button {
            Button::Left => 1,
            Button::Middle => 2,
            Button::Right => 3,
        };
        self.click(detail, down)?;
        self.conn.flush()?;
        Ok(())
    }

    fn wheel(&mut self, vertical: i32, horizontal: i32) -> Result<(), Error> {
        // ホイールはボタン 4（上）・5（下）・6（左）・7（右）の押下と解放として送る。
        for (count, positive, negative) in [(vertical, 4, 5), (horizontal, 7, 6)] {
            let detail = if count > 0 { positive } else { negative };
            for _ in 0..count.unsigned_abs().min(20) {
                self.click(detail, true)?;
                self.click(detail, false)?;
            }
        }
        self.conn.flush()?;
        Ok(())
    }
}
