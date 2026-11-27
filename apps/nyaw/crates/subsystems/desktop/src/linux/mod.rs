//! Linux の画面の取り込みと入力注入。表示サーバーに応じて X11（XSHM / XTest）、wlroots（wlr-screencopy /
//! 仮想ポインタ）、その他の Wayland（xdg-desktop-portal の ScreenCast・RemoteDesktop と PipeWire）を使い分ける。
//! 取り込んだ画像は、ランタイムが取り込める限り CPU で並べ替えずに GPU へ渡す（`vr::ExternalImage`）。

pub(super) mod apps;
mod keyboard;
mod pipewire;
mod portal;
mod wayland;
mod wlr;
mod x11;

use std::{
    os::fd::{AsFd, OwnedFd},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use rustix::{fs, mm};
use vr::{ExternalImage, ImportFormats, Memory, PixelFormat};

use super::{Button, Capabilities, Cursor, Error, Frame, Session, Source, Touch, TouchPhase, Wake};

pub(super) const CAPABILITIES: Capabilities = Capabilities {
    hover: true,
    minimize: false,
};

/// 取り込みに使う表示サーバーの仕組み。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    X11,
    Wlr,
    Portal,
}

/// 環境変数 `NYAW_CAPTURE`（`x11` / `wlr` / `portal`）で選ぶ。指定が無ければ、Wayland で wlr-screencopy を
/// 提供していれば wlroots、提供していなければ portal、Wayland でなければ X11 を使う。
fn backend() -> Result<Backend, Error> {
    match std::env::var("NYAW_CAPTURE").as_deref() {
        Ok("x11") => return Ok(Backend::X11),
        Ok("wlr") => return Ok(Backend::Wlr),
        Ok("portal") => return Ok(Backend::Portal),
        _ => {}
    }
    let set = |key| std::env::var_os(key).is_some_and(|v| !v.is_empty());
    if set("WAYLAND_DISPLAY") {
        return Ok(if wayland::has_screencopy() {
            Backend::Wlr
        } else {
            Backend::Portal
        });
    }
    if set("DISPLAY") {
        return Ok(Backend::X11);
    }
    Err(Error(super::failure::NO_DISPLAY_SERVER.into()))
}

/// OS キーボードの送信先（[`crate::os_keyboard`]）。
pub(super) fn keyboard() -> keyboard::Keyboard {
    keyboard::Keyboard::new()
}

pub(super) fn sources() -> Result<Vec<Source>, Error> {
    match backend()? {
        Backend::X11 => x11::sources(),
        Backend::Wlr | Backend::Portal => wayland::sources(),
    }
}

pub(super) fn open(
    source: Source,
    wake: Wake,
    import: Option<ImportFormats>,
) -> Result<Box<dyn Session>, Error> {
    Ok(match backend()? {
        Backend::X11 => Box::new(x11::Capture::new(source, wake, import)?),
        Backend::Wlr => Box::new(wlr::Capture::new(source, wake, import)?),
        Backend::Portal => Box::new(portal::Capture::new(source, wake, import)?),
    })
}

impl Session for Box<dyn Session> {
    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        (**self).frame()
    }
    fn skip(&mut self) -> Result<(), Error> {
        (**self).skip()
    }
    fn touch(&mut self, touch: Touch) -> Result<(), Error> {
        (**self).touch(touch)
    }
    fn button(&mut self, touch: Touch, button: Button) -> Result<(), Error> {
        (**self).button(touch, button)
    }
    fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), Error> {
        (**self).scroll(x, y, dx, dy)
    }
    fn heartbeat(&mut self) -> Result<(), Error> {
        (**self).heartbeat()
    }
    fn closed(&self) -> bool {
        (**self).closed()
    }
    fn cursor(&mut self) -> Option<Arc<Cursor>> {
        (**self).cursor()
    }
    fn pointer(&mut self) -> Option<(f32, f32)> {
        (**self).pointer()
    }
}

/// 画面を表示名で区別する。製品名（取れなければコネクタ名）を使い、同じ名前が複数あればコネクタ名を添える。
fn label(screens: &[(String, Option<String>)]) -> Vec<String> {
    screens
        .iter()
        .map(|(connector, product)| match product {
            Some(p)
                if screens
                    .iter()
                    .filter(|(_, q)| q.as_ref() == Some(p))
                    .count()
                    > 1 =>
            {
                format!("{p} ({connector})")
            }
            Some(p) => p.clone(),
            None => connector.clone(),
        })
        .collect()
}

/// EDID の表示名（記述子 0xFC）。
fn edid_name(edid: &[u8]) -> Option<String> {
    if edid.len() < 128 || edid[..8] != [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0] {
        return None;
    }
    (0..4)
        .map(|i| &edid[54 + i * 18..72 + i * 18])
        .find(|d| d[..3] == [0, 0, 0] && d[3] == 0xfc)
        .map(|d| {
            String::from_utf8_lossy(&d[5..])
                .split('\n')
                .next()
                .unwrap_or("")
                .trim()
                .to_owned()
        })
        .filter(|s| !s.is_empty())
}

/// 書き込み元（表示サーバー）と共有するメモリ。ランタイムがこの領域を読み終えるまで（[`ExternalImage`] の破棄まで）
/// 次の書き込みに使わない。
struct Shm {
    fd: OwnedFd,
    ptr: *mut u8,
    len: usize,
    free: AtomicBool,
}

// 書き込みは `free` の間だけ、読み出しは取り込みの間だけ行い、同時には起きない。
unsafe impl Send for Shm {}
unsafe impl Sync for Shm {}

impl Shm {
    /// `size` バイト以上を、ページ（とランタイムの取り込みの境界）に揃えて確保する。
    fn new(size: usize, import: Option<&ImportFormats>) -> Result<Arc<Self>, Error> {
        let align = import
            .and_then(|i| i.host_alignment)
            .unwrap_or(1)
            .max(rustix::param::page_size());
        let len = size.div_ceil(align) * align;
        let err = |what: &'static str| move |e: rustix::io::Errno| Error(format!("{what}: {e}"));
        let fd = fs::memfd_create(c"nyaw-desktop", fs::MemfdFlags::CLOEXEC)
            .map_err(err("memfd_create"))?;
        fs::ftruncate(&fd, len as u64).map_err(err("ftruncate"))?;
        let ptr = unsafe {
            mm::mmap(
                std::ptr::null_mut(),
                len,
                mm::ProtFlags::READ | mm::ProtFlags::WRITE,
                mm::MapFlags::SHARED,
                &fd,
                0,
            )
        }
        .map_err(err("mmap"))? as *mut u8;
        Ok(Arc::new(Self {
            fd,
            ptr,
            len,
            free: AtomicBool::new(true),
        }))
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.fd.as_fd()
    }

    fn free(&self) -> bool {
        self.free.load(Ordering::Acquire)
    }

    /// `width`×`height`（`stride` バイト/行）の `format` の画像をフレームにする。ランタイムが共有メモリを取り込めれば
    /// そのまま渡し（読み終えるまで書き込みに使わない）、取り込めなければ RGBA へ並べ替える。
    fn frame(
        self: &Arc<Self>,
        width: u32,
        height: u32,
        stride: u32,
        format: PixelFormat,
        flipped: bool,
        import: Option<&ImportFormats>,
    ) -> Result<Frame, Error> {
        if !flipped && import.is_some_and(|i| i.host(self.ptr, self.len)) {
            self.free.store(false, Ordering::Release);
            let held = self.clone();
            return Ok(Frame::external(ExternalImage::new(
                width,
                height,
                format,
                Memory::Host {
                    ptr: self.ptr,
                    len: self.len,
                    offset: 0,
                    stride,
                },
                move || held.free.store(true, Ordering::Release),
            )));
        }
        let mut frame = Frame::from_pixels(width, height, stride as usize, self.bytes(), format)?;
        if flipped {
            let row = width as usize * 4;
            let rows: Vec<&[u8]> = frame.rgba.chunks_exact(row).rev().collect();
            frame.rgba = rows.concat();
        }
        Ok(frame)
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        let _ = unsafe { mm::munmap(self.ptr.cast(), self.len) };
    }
}

/// OS へポインタの操作を送る手段（表示サーバーごと）。座標は取り込み元の左上原点、0〜1。
trait Inject {
    fn motion(&mut self, x: f32, y: f32) -> Result<(), Error>;
    fn press(&mut self, button: Button, down: bool) -> Result<(), Error>;
    /// ホイールのノッチ数（上・右が正）。
    fn wheel(&mut self, vertical: i32, horizontal: i32) -> Result<(), Error>;
}

/// 左右の手の接点を、OS のマウス 1 台の操作に変換する（Windows の `MouseDevice` と同じ規則）。先に押した手だけが
/// ボタンを持ち、離すまでもう片方の手の押下は送らない。
#[derive(Debug, Default)]
struct Pointer {
    pressed_by: Option<u32>,
    button: Option<Button>,
    /// 1 ノッチ未満のホイール量の蓄積値（縦, 横）。
    wheel: [f32; 2],
}

impl Pointer {
    fn send(&mut self, out: &mut dyn Inject, touch: Touch, button: Button) -> Result<(), Error> {
        let id = touch.id;
        match touch.phase {
            TouchPhase::Move if self.pressed_by.is_none() || self.pressed_by == Some(id) => {
                out.motion(touch.x, touch.y)?;
            }
            TouchPhase::Down if self.pressed_by.is_none() => {
                out.motion(touch.x, touch.y)?;
                out.press(button, true)?;
                self.pressed_by = Some(id);
                self.button = Some(button);
            }
            TouchPhase::Up | TouchPhase::Cancel if self.pressed_by == Some(id) => {
                if touch.phase == TouchPhase::Up {
                    out.motion(touch.x, touch.y)?;
                }
                out.press(self.button.unwrap_or(Button::Left), false)?;
                self.pressed_by = None;
                self.button = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn scroll(
        &mut self,
        out: &mut dyn Inject,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> Result<(), Error> {
        out.motion(x, y)?;
        let vertical = wheel_notches(&mut self.wheel[0], dy);
        let horizontal = wheel_notches(&mut self.wheel[1], dx);
        if vertical != 0 || horizontal != 0 {
            out.wheel(vertical, horizontal)?;
        }
        Ok(())
    }

    /// 押したまま終わる場合に離す。
    fn release(&mut self, out: &mut dyn Inject) {
        if let Some(button) = self.button.take() {
            let _ = out.press(button, false);
        }
        self.pressed_by = None;
    }
}

/// スティックの入力量を蓄積し、送るノッチ数を返す（3 で 1 ノッチ。Windows の `wheel_notches` と同じ量）。
fn wheel_notches(acc: &mut f32, delta: f32) -> i32 {
    let amount = delta / 3.0;
    // 方向が反転した場合、逆方向の蓄積値を即座に破棄する
    if *acc * amount < 0.0 {
        *acc = 0.0;
    }
    *acc += amount;
    let notches = acc.trunc();
    *acc -= notches;
    notches as i32
}

/// 0〜1 の座標を、`size` 画素の範囲の画素の位置にする（右端・下端は最後の画素）。
fn pixel(v: f32, size: u32) -> i32 {
    (size.saturating_sub(1) as f32 * v.clamp(0.0, 1.0)).round() as i32
}

/// 取り込み元の画素の位置 (`x`, `y`) を 0〜1 にする（[`pixel`] の逆）。範囲の外なら `None`。
fn normalized(x: i32, y: i32, width: u32, height: u32) -> Option<(f32, f32)> {
    let inside = |v: i32, size: u32| v >= 0 && (v as u32) < size;
    if !inside(x, width) || !inside(y, height) {
        return None;
    }
    let scale = |v: i32, size: u32| v as f32 / size.saturating_sub(1).max(1) as f32;
    Some((scale(x, width), scale(y, height)))
}

/// 事前乗算済みの ARGB（XFixes・PipeWire のカーソル）を、ストレートアルファの RGBA にする。
fn unpremultiply_argb(width: u32, height: u32, argb: impl Iterator<Item = u32>) -> super::Icon {
    let rgba: Vec<u8> = argb
        .flat_map(|p| {
            let a = p >> 24;
            let c = |shift: u32| {
                let v = (p >> shift) & 0xff;
                (v * 255).checked_div(a).map_or(0, |v| v.min(255) as u8)
            };
            [c(16), c(8), c(0), a as u8]
        })
        .collect();
    super::Icon {
        width,
        height,
        rgba: rgba.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder(Vec<String>);
    impl Inject for Recorder {
        fn motion(&mut self, x: f32, y: f32) -> Result<(), Error> {
            self.0.push(format!("move {x} {y}"));
            Ok(())
        }
        fn press(&mut self, button: Button, down: bool) -> Result<(), Error> {
            self.0.push(format!("{button:?} {down}"));
            Ok(())
        }
        fn wheel(&mut self, vertical: i32, horizontal: i32) -> Result<(), Error> {
            self.0.push(format!("wheel {vertical} {horizontal}"));
            Ok(())
        }
    }

    fn touch(id: u32, phase: TouchPhase, x: f32) -> Touch {
        Touch {
            id,
            phase,
            x,
            y: 0.5,
        }
    }

    /// 先に押した手だけがボタンを持ち、もう片方の手は離すまで押せず動かせない。
    #[test]
    fn the_first_hand_to_press_owns_the_button() {
        let (mut p, mut out) = (Pointer::default(), Recorder::default());
        p.send(&mut out, touch(0, TouchPhase::Down, 0.1), Button::Right)
            .unwrap();
        p.send(&mut out, touch(1, TouchPhase::Down, 0.9), Button::Left)
            .unwrap();
        p.send(&mut out, touch(1, TouchPhase::Move, 0.8), Button::Left)
            .unwrap();
        p.send(&mut out, touch(0, TouchPhase::Up, 0.2), Button::Left)
            .unwrap();
        p.send(&mut out, touch(1, TouchPhase::Move, 0.7), Button::Left)
            .unwrap();
        assert_eq!(
            out.0,
            [
                "move 0.1 0.5",
                "Right true",
                "move 0.2 0.5",
                "Right false",
                "move 0.7 0.5"
            ]
        );
    }

    #[test]
    fn cancel_releases_without_moving_and_drop_releases_a_held_button() {
        let (mut p, mut out) = (Pointer::default(), Recorder::default());
        p.send(&mut out, touch(0, TouchPhase::Down, 0.1), Button::Left)
            .unwrap();
        p.send(&mut out, touch(0, TouchPhase::Cancel, 0.9), Button::Left)
            .unwrap();
        p.send(&mut out, touch(1, TouchPhase::Down, 0.3), Button::Middle)
            .unwrap();
        p.release(&mut out);
        p.release(&mut out);
        assert_eq!(
            out.0,
            [
                "move 0.1 0.5",
                "Left true",
                "Left false",
                "move 0.3 0.5",
                "Middle true",
                "Middle false"
            ]
        );
    }

    #[test]
    fn wheel_sends_whole_notches_and_drops_the_other_direction() {
        let mut acc = 0.0;
        let sent: Vec<_> = (0..6).map(|_| wheel_notches(&mut acc, 1.0)).collect();
        assert_eq!(sent, [0, 0, 1, 0, 0, 1]);
        let mut acc = 0.0;
        wheel_notches(&mut acc, 2.0);
        assert_eq!(wheel_notches(&mut acc, -3.0), -1);
        let (mut p, mut out) = (Pointer::default(), Recorder::default());
        p.scroll(&mut out, 0.5, 0.5, -6.0, 3.0).unwrap();
        assert_eq!(out.0, ["move 0.5 0.5", "wheel 1 -2"]);
    }

    #[test]
    fn pixels_reach_the_last_row_and_column() {
        assert_eq!(pixel(0.0, 1920), 0);
        assert_eq!(pixel(1.0, 1920), 1919);
        assert_eq!(pixel(2.0, 1080), 1079);
        // 位置へ戻すと同じ画素を指す。範囲の外は無い。
        let (x, y) = normalized(1919, 540, 1920, 1080).unwrap();
        assert_eq!((pixel(x, 1920), pixel(y, 1080)), (1919, 540));
        assert_eq!(normalized(-1, 0, 1920, 1080), None);
        assert_eq!(normalized(0, 1080, 1920, 1080), None);
    }

    #[test]
    fn screens_are_named_after_their_products() {
        let screens = [
            ("DP-1".to_owned(), Some("DELL U2720Q".to_owned())),
            ("DP-2".to_owned(), Some("DELL U2720Q".to_owned())),
            ("HDMI-A-1".to_owned(), Some("LG HDR 4K".to_owned())),
            ("eDP-1".to_owned(), None),
        ];
        assert_eq!(
            label(&screens),
            [
                "DELL U2720Q (DP-1)",
                "DELL U2720Q (DP-2)",
                "LG HDR 4K",
                "eDP-1"
            ]
        );
    }

    #[test]
    fn reads_the_monitor_name_from_edid() {
        let mut edid = vec![0u8; 128];
        edid[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        let d = 54 + 18;
        edid[d + 3] = 0xfc;
        edid[d + 5..d + 18].copy_from_slice(b"DELL U2720Q\n ");
        assert_eq!(edid_name(&edid).as_deref(), Some("DELL U2720Q"));
        edid[d + 3] = 0xff;
        assert_eq!(edid_name(&edid), None);
        assert_eq!(edid_name(&[0; 10]), None);
    }

    #[test]
    fn cursor_pixels_become_straight_alpha() {
        let icon = unpremultiply_argb(2, 1, [0x80402010, 0].into_iter());
        assert_eq!(&*icon.rgba, [127, 63, 31, 128, 0, 0, 0, 0]);
    }

    /// 取り込めない共有メモリは RGBA に並べ替え、透明度の無い形式は不透明に、上下反転は戻す。
    #[test]
    fn shared_memory_becomes_rgba_without_an_importer() {
        let shm = Shm::new(16, None).unwrap();
        unsafe {
            std::ptr::copy_nonoverlapping([1u8, 2, 3, 0, 5, 6, 7, 0].as_ptr(), shm.ptr, 8);
        }
        let frame = shm.frame(1, 2, 4, PixelFormat::Bgrx, true, None).unwrap();
        assert_eq!(frame.rgba, [7, 6, 5, 255, 3, 2, 1, 255]);
        assert!(frame.external.is_none() && shm.free());
    }

    /// 取り込める共有メモリはそのまま渡し、ランタイムが手放すまで次の書き込みに使わない。
    #[test]
    fn shared_memory_is_lent_to_the_runtime_until_it_is_dropped() {
        let import = ImportFormats {
            host_alignment: Some(4096),
            ..Default::default()
        };
        let shm = Shm::new(100, Some(&import)).unwrap();
        assert_eq!(shm.len, 4096);
        let frame = shm
            .frame(5, 5, 20, PixelFormat::Bgra, false, Some(&import))
            .unwrap();
        assert!(frame.rgba.is_empty() && frame.external.is_some());
        assert!(!shm.free());
        drop(frame);
        assert!(shm.free());
        // 上下反転した画像は GPU へ渡さず、並べ替えで戻す。
        let frame = shm
            .frame(5, 5, 20, PixelFormat::Bgra, true, Some(&import))
            .unwrap();
        assert!(frame.external.is_none() && shm.free());
    }
}
