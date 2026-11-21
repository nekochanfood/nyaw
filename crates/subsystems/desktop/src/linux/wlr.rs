//! wlroots 系のコンポジタ（sway、Hyprland、river、labwc など）: 取り込みは wlr-screencopy、入力は wlr の仮想ポインタで行う。
//! 取り込みは専用のスレッドがイベントを処理し、画面が変化したとき（`copy_with_damage`）だけ共有メモリへ写させる。

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    protocol::{wl_buffer, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool},
};
use wayland_protocols_wlr::{
    screencopy::v1::client::{zwlr_screencopy_frame_v1 as frame, zwlr_screencopy_manager_v1},
    virtual_pointer::v1::client::{zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1},
};

use super::{
    Button, Error, Frame, ImportFormats, Inject, PixelFormat, Pointer, Session, Shm, Source, Touch,
    Wake,
};

/// 写し先の共有メモリの数（コンポジタが書き込み中・最新・ランタイムが転送中）。
const SHM_COUNT: usize = 3;

/// 写し終えた画像。
struct Done {
    shm: Arc<Shm>,
    width: u32,
    height: u32,
    stride: u32,
    format: PixelFormat,
    flipped: bool,
    captured: Instant,
}

/// 取り込みのスレッドとワーカーが共有する状態。
#[derive(Default)]
struct Shared {
    latest: Mutex<Option<Done>>,
    /// 画像の大きさ（仮想ポインタの座標の範囲）。
    size: Mutex<(u32, u32)>,
    closed: AtomicBool,
    stop: AtomicBool,
}

/// 写し先の候補（`buffer` イベント）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    format: wl_shm::Format,
    width: u32,
    height: u32,
    stride: u32,
}

struct State {
    shared: Arc<Shared>,
    wake: Wake,
    import: Option<ImportFormats>,
    output_global: u32,
    output: Option<wl_output::WlOutput>,
    seat: Option<wl_seat::WlSeat>,
    shm: Option<wl_shm::WlShm>,
    manager: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    pointers: Option<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
    /// 共有メモリと、それを指す wl_buffer。
    ring: Vec<(Arc<Shm>, wl_buffer::WlBuffer, Layout)>,
    /// 写している最中のフレームの候補と、使っている共有メモリ。
    offered: Option<Layout>,
    copying: Option<(Arc<Shm>, Layout)>,
    flipped: bool,
    /// 取り込みを要求中（ready / failed を待っている）。
    pending: bool,
    failures: u32,
}

fn pixel_format(format: wl_shm::Format) -> Option<PixelFormat> {
    Some(match format {
        wl_shm::Format::Argb8888 => PixelFormat::Bgra,
        wl_shm::Format::Xrgb8888 => PixelFormat::Bgrx,
        wl_shm::Format::Abgr8888 => PixelFormat::Rgba,
        wl_shm::Format::Xbgr8888 => PixelFormat::Rgbx,
        _ => return None,
    })
}

impl State {
    /// 候補 `layout` に合う、使っていない wl_buffer（無ければ作る。全部使用中なら `None`）。
    fn buffer(
        &mut self,
        layout: Layout,
        qh: &QueueHandle<Self>,
    ) -> Option<(Arc<Shm>, wl_buffer::WlBuffer)> {
        if self.ring.first().is_some_and(|(_, _, l)| *l != layout) {
            for (_, buffer, _) in self.ring.drain(..) {
                buffer.destroy();
            }
        }
        if let Some((shm, buffer, _)) = self.ring.iter().find(|(s, ..)| s.free()) {
            return Some((shm.clone(), buffer.clone()));
        }
        if self.ring.len() >= SHM_COUNT {
            return None;
        }
        let size = layout.stride as usize * layout.height as usize;
        let shm = Shm::new(size, self.import.as_ref())
            .inspect_err(|e| logger::warn!("cannot allocate a screencopy buffer", error = e))
            .ok()?;
        let pool = self
            .shm
            .as_ref()?
            .create_pool(shm.fd(), shm.len as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            layout.width as i32,
            layout.height as i32,
            layout.stride as i32,
            layout.format,
            qh,
            (),
        );
        pool.destroy();
        self.ring.push((shm.clone(), buffer.clone(), layout));
        Some((shm, buffer))
    }

    /// 候補が揃ったら共有メモリへ写させる。
    fn copy(&mut self, frame: &frame::ZwlrScreencopyFrameV1, qh: &QueueHandle<Self>) {
        let Some(layout) = self.offered.take() else {
            frame.destroy();
            self.pending = false;
            return;
        };
        let Some((shm, buffer)) = self.buffer(layout, qh) else {
            frame.destroy();
            self.pending = false;
            return;
        };
        shm.free.store(false, Ordering::Release);
        if frame.version() >= 2 {
            frame.copy_with_damage(&buffer);
        } else {
            frame.copy(&buffer);
        }
        self.copying = Some((shm, layout));
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_output" if name == state.output_global => {
                    state.output = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(7), qh, ()));
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "zwlr_screencopy_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(3), qh, ()));
                }
                "zwlr_virtual_pointer_manager_v1" => {
                    state.pointers = Some(registry.bind(name, version.min(2), qh, ()));
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } if name == state.output_global => {
                state.shared.closed.store(true, Ordering::Release);
                state.wake.wake();
            }
            _ => {}
        }
    }
}

impl Dispatch<frame::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &frame::ZwlrScreencopyFrameV1,
        event: frame::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            frame::Event::Buffer {
                format: WEnum::Value(format),
                width,
                height,
                stride,
            } => {
                if state.offered.is_none() && pixel_format(format).is_some() {
                    state.offered = Some(Layout {
                        format,
                        width,
                        height,
                        stride,
                    });
                }
                // 第 3 版より前は buffer_done が無く、buffer が届いた時点で写させる。
                if proxy.version() < 3 {
                    state.copy(proxy, qh);
                }
            }
            frame::Event::BufferDone => state.copy(proxy, qh),
            frame::Event::Flags {
                flags: WEnum::Value(flags),
            } => state.flipped = flags.contains(frame::Flags::YInvert),
            frame::Event::Ready {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
            } => {
                proxy.destroy();
                state.pending = false;
                state.failures = 0;
                let Some((shm, layout)) = state.copying.take() else {
                    return;
                };
                let shown =
                    Duration::new((u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo), tv_nsec);
                let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
                let now = Duration::new(now.tv_sec as u64, now.tv_nsec as u32);
                let captured = Instant::now()
                    .checked_sub(now.saturating_sub(shown))
                    .unwrap_or_else(Instant::now);
                *state.shared.size.lock().unwrap() = (layout.width, layout.height);
                let done = Done {
                    shm,
                    width: layout.width,
                    height: layout.height,
                    stride: layout.stride,
                    format: pixel_format(layout.format).unwrap_or(PixelFormat::Bgrx),
                    flipped: state.flipped,
                    captured,
                };
                // 取りに来なかった前の画像は、次に写す先として空ける。
                if let Some(old) = state.shared.latest.lock().unwrap().replace(done) {
                    old.shm.free.store(true, Ordering::Release);
                }
                state.wake.wake();
            }
            frame::Event::Failed => {
                proxy.destroy();
                state.pending = false;
                if let Some((shm, _)) = state.copying.take() {
                    shm.free.store(true, Ordering::Release);
                }
                state.failures += 1;
                if state.failures >= 10 {
                    state.shared.closed.store(true, Ordering::Release);
                    state.wake.wake();
                }
            }
            _ => {}
        }
    }
}

/// 要求のみでイベントを受け取らない（または使わない）オブジェクト。
macro_rules! ignore_events {
    ($($proxy:ty),* $(,)?) => {$(
        impl Dispatch<$proxy, ()> for State {
            fn event(_: &mut Self, _: &$proxy, _: <$proxy as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_events!(
    wl_output::WlOutput,
    wl_seat::WlSeat,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
);

/// コンポジタのイベントを処理し、画面の変化を待って写させ続ける。
fn run(conn: &Connection, mut queue: EventQueue<State>, mut state: State) {
    let qh = queue.handle();
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: 8_000_000,
    };
    while !state.shared.stop.load(Ordering::Acquire) && !state.shared.closed.load(Ordering::Acquire)
    {
        let room = state.ring.len() < SHM_COUNT || state.ring.iter().any(|(s, ..)| s.free());
        if !state.pending
            && room
            && let (Some(manager), Some(output)) = (&state.manager, &state.output)
        {
            // カーソルは映像に含めない（VR のポインタが代わりに示す）。
            manager.capture_output(0, output, &qh, ());
            state.pending = true;
        }
        if queue.flush().is_err() {
            break;
        }
        let Some(guard) = queue.prepare_read() else {
            if queue.dispatch_pending(&mut state).is_err() {
                break;
            }
            continue;
        };
        let fd = guard.connection_fd();
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        let ready = poll(&mut fds, Some(&timeout)).unwrap_or(0) > 0;
        if ready {
            if guard.read().is_err() {
                break;
            }
        } else {
            drop(guard);
        }
        if queue.dispatch_pending(&mut state).is_err() {
            break;
        }
    }
    for (_, buffer, _) in state.ring.drain(..) {
        buffer.destroy();
    }
    let _ = conn.flush();
    state.shared.closed.store(true, Ordering::Release);
    state.wake.wake();
}

pub(super) struct Capture {
    conn: Connection,
    shared: Arc<Shared>,
    pointer: Option<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1>,
    buttons: Pointer,
    import: Option<ImportFormats>,
    started: Instant,
    worker: Option<thread::JoinHandle<()>>,
}

impl Capture {
    pub fn new(source: Source, wake: Wake, import: Option<ImportFormats>) -> Result<Self, Error> {
        let conn = Connection::connect_to_env()?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let shared = Arc::new(Shared::default());
        let mut state = State {
            shared: shared.clone(),
            wake,
            import: import.clone(),
            output_global: u32::try_from(source.target).unwrap_or(0),
            output: None,
            seat: None,
            shm: None,
            manager: None,
            pointers: None,
            ring: Vec::new(),
            offered: None,
            copying: None,
            flipped: false,
            pending: false,
            failures: 0,
        };
        queue.roundtrip(&mut state)?;
        if state.manager.is_none() {
            return Err(Error(crate::failure::UNSUPPORTED.into()));
        }
        let Some(output) = state.output.clone() else {
            return Err(Error(crate::failure::NO_PIXELS.into()));
        };
        let pointer = match (&state.pointers, &state.seat) {
            (Some(manager), seat) if manager.version() >= 2 => Some(
                manager.create_virtual_pointer_with_output(seat.as_ref(), Some(&output), &qh, ()),
            ),
            _ => {
                logger::warn!("this compositor has no virtual pointer; the screen is view-only");
                None
            }
        };
        let worker = {
            let conn = conn.clone();
            thread::Builder::new()
                .name("desktop-wlr".into())
                .spawn(move || run(&conn, queue, state))
                .map_err(|e| Error(e.to_string()))?
        };
        Ok(Self {
            conn,
            shared,
            pointer,
            buttons: Pointer::default(),
            import,
            started: Instant::now(),
            worker: Some(worker),
        })
    }

    fn inject(&mut self) -> Option<VirtualPointer<'_>> {
        Some(VirtualPointer {
            conn: &self.conn,
            pointer: self.pointer.as_ref()?,
            size: *self.shared.size.lock().unwrap(),
            started: self.started,
        })
    }
}

impl Session for Capture {
    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        let Some(done) = self.shared.latest.lock().unwrap().take() else {
            return Ok(None);
        };
        let frame = done.shm.frame(
            done.width,
            done.height,
            done.stride,
            done.format,
            done.flipped,
            self.import.as_ref(),
        );
        // ランタイムへ渡さず並べ替えた（または失敗した）場合は、すぐに次の写し先として空ける。
        if frame.as_ref().is_ok_and(|f| f.external.is_none()) || frame.is_err() {
            done.shm.free.store(true, Ordering::Release);
        }
        let mut frame = frame?;
        frame.captured = done.captured;
        Ok(Some(frame))
    }

    fn closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }

    fn touch(&mut self, touch: Touch) -> Result<(), Error> {
        self.button(touch, Button::Left)
    }

    fn button(&mut self, touch: Touch, button: Button) -> Result<(), Error> {
        let mut buttons = std::mem::take(&mut self.buttons);
        let result = match self.inject() {
            Some(mut out) => buttons.send(&mut out, touch, button),
            None => Ok(()),
        };
        self.buttons = buttons;
        result
    }

    fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), Error> {
        let mut buttons = std::mem::take(&mut self.buttons);
        let result = match self.inject() {
            Some(mut out) => buttons.scroll(&mut out, x, y, dx, dy),
            None => Ok(()),
        };
        self.buttons = buttons;
        result
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let mut buttons = std::mem::take(&mut self.buttons);
        if let Some(mut out) = self.inject() {
            buttons.release(&mut out);
        }
        if let Some(pointer) = self.pointer.take() {
            pointer.destroy();
        }
        self.shared.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// 取り込み元の出力に結び付けた仮想ポインタ。座標は出力の左上原点の画素。
struct VirtualPointer<'a> {
    conn: &'a Connection,
    pointer: &'a zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    size: (u32, u32),
    started: Instant,
}

impl VirtualPointer<'_> {
    fn time(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    fn flush(&self) -> Result<(), Error> {
        self.conn
            .flush()
            .map_err(|e| Error(format!("Wayland: {e}")))
    }
}

/// evdev のボタン番号（linux/input-event-codes.h）。
fn evdev_button(button: Button) -> u32 {
    match button {
        Button::Left => 0x110,
        Button::Right => 0x111,
        Button::Middle => 0x112,
    }
}

impl Inject for VirtualPointer<'_> {
    fn motion(&mut self, x: f32, y: f32) -> Result<(), Error> {
        // 大きさが分からないうちは 0〜9999 の範囲で送る（出力に対する割合で解釈される）。
        let (w, h) = match self.size {
            (0, _) | (_, 0) => (10_000, 10_000),
            size => size,
        };
        let px = super::pixel(x, w) as u32;
        let py = super::pixel(y, h) as u32;
        self.pointer.motion_absolute(self.time(), px, py, w, h);
        self.pointer.frame();
        self.flush()
    }

    fn press(&mut self, button: Button, down: bool) -> Result<(), Error> {
        let state = if down {
            wl_pointer::ButtonState::Pressed
        } else {
            wl_pointer::ButtonState::Released
        };
        self.pointer
            .button(self.time(), evdev_button(button), state);
        self.pointer.frame();
        self.flush()
    }

    fn wheel(&mut self, vertical: i32, horizontal: i32) -> Result<(), Error> {
        let time = self.time();
        self.pointer.axis_source(wl_pointer::AxisSource::Wheel);
        // Wayland の軸は下・右が正。1 ノッチを 15 として送る（libinput の既定）。
        for (axis, steps) in [
            (wl_pointer::Axis::VerticalScroll, -vertical),
            (wl_pointer::Axis::HorizontalScroll, horizontal),
        ] {
            if steps != 0 {
                self.pointer
                    .axis_discrete(time, axis, f64::from(steps * 15), steps);
            }
        }
        self.pointer.frame();
        self.flush()
    }
}
