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
use wayland_protocols::ext::{
    image_capture_source::v1::client::{
        ext_image_capture_source_v1 as capture_source,
        ext_output_image_capture_source_manager_v1 as capture_sources,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_cursor_session_v1 as cursor_session,
        ext_image_copy_capture_frame_v1 as ext_frame, ext_image_copy_capture_manager_v1 as copier,
        ext_image_copy_capture_session_v1 as copy_session,
    },
};
use wayland_protocols_wlr::{
    screencopy::v1::client::{zwlr_screencopy_frame_v1 as frame, zwlr_screencopy_manager_v1},
    virtual_pointer::v1::client::{zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1},
};

use super::{
    Button, Cursor, Error, Frame, ImportFormats, Inject, PixelFormat, Pointer, Session, Shm,
    Source, Touch, Wake,
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
    /// 後継の ext-image-copy-capture のカーソル用セッションが知らせるカーソルと位置。
    cursor: Mutex<Option<Arc<Cursor>>>,
    pointer: Mutex<Option<(f32, f32)>>,
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
    /// 写し終えた時刻（ext の presentation_time）。
    shown: Option<Duration>,
    ext: Ext,
}

/// 後継の ext-image-copy-capture-v1 の状態。提供していれば、画面とカーソルをこちらで取り込む。
#[derive(Default)]
struct Ext {
    copier: Option<copier::ExtImageCopyCaptureManagerV1>,
    sources: Option<capture_sources::ExtOutputImageCaptureSourceManagerV1>,
    screen: Option<copy_session::ExtImageCopyCaptureSessionV1>,
    cursor: Option<copy_session::ExtImageCopyCaptureSessionV1>,
    /// 用途ごとの、知らされた大きさと形式、done で確定した写し先。
    size: [Option<(u32, u32)>; 2],
    format: [Option<wl_shm::Format>; 2],
    layout: [Option<Layout>; 2],
    cursor_pending: bool,
    cursor_buffer: Option<(Arc<Shm>, wl_buffer::WlBuffer, Layout)>,
    hotspot: (u32, u32),
    image: Option<crate::Icon>,
    visible: bool,
    /// 後継で開始した時刻と、最初の画面が届いたか（届かなければ wlr-screencopy に切り替える）。
    started: Option<Instant>,
    delivered: bool,
}

/// 後継で開始してから最初の画面を待つ時間。過ぎても届かず wlr-screencopy があれば、そちらに切り替える
/// （ShojiWM では後継の取り込みが完了しなかった）。
const EXT_FIRST_FRAME: Duration = Duration::from_secs(2);

/// 取り込みのセッションの用途（[`Ext`] の添字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Screen = 0,
    Cursor = 1,
}

fn force_wlr() -> bool {
    std::env::var("NYAW_SCREENCOPY").as_deref() == Ok("wlr")
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

impl State {
    /// 写し終えた画面を最新の画像にする（wlr-screencopy と ext-image-copy-capture で共通）。
    fn deliver(&mut self) {
        self.pending = false;
        self.failures = 0;
        let Some((shm, layout)) = self.copying.take() else {
            return;
        };
        let captured = self
            .shown
            .take()
            .and_then(|shown| {
                let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
                let now = Duration::new(now.tv_sec as u64, now.tv_nsec as u32);
                // 提示時刻を CLOCK_MONOTONIC 以外で送るコンポジタ（ShojiWM）もある。1 秒より古ければ信用しない。
                let age = now
                    .checked_sub(shown)
                    .filter(|a| *a < Duration::from_secs(1))?;
                Instant::now().checked_sub(age)
            })
            .unwrap_or_else(Instant::now);
        *self.shared.size.lock().unwrap() = (layout.width, layout.height);
        let done = Done {
            shm,
            width: layout.width,
            height: layout.height,
            stride: layout.stride,
            format: pixel_format(layout.format).unwrap_or(PixelFormat::Bgrx),
            flipped: self.flipped,
            captured,
        };
        // 取りに来なかった前の画像は、次に写す先として空ける。
        if let Some(old) = self.shared.latest.lock().unwrap().replace(done) {
            old.shm.free.store(true, Ordering::Release);
        }
        self.wake.wake();
    }

    fn failed(&mut self) {
        self.pending = false;
        if let Some((shm, _)) = self.copying.take() {
            shm.free.store(true, Ordering::Release);
        }
        self.failures += 1;
        if self.failures >= 10 {
            self.shared.closed.store(true, Ordering::Release);
            self.wake.wake();
        }
    }

    /// 後継のプロトコルで、画面とカーソルの取り込みを始める。
    fn start_ext(&mut self, qh: &QueueHandle<Self>) -> bool {
        let (Some(copier), Some(sources), Some(output)) =
            (&self.ext.copier, &self.ext.sources, &self.output)
        else {
            return false;
        };
        self.ext.started = Some(Instant::now());
        let source = sources.create_source(output, qh, ());
        // カーソルは映像に含めず、カーソル用のセッションで別に受け取る。
        self.ext.screen =
            Some(copier.create_session(&source, copier::Options::empty(), qh, Role::Screen));
        if let Some(seat) = &self.seat {
            let pointer = seat.get_pointer(qh, ());
            let cursor = copier.create_pointer_cursor_session(&source, &pointer, qh, ());
            self.ext.cursor = Some(cursor.get_capture_session(qh, Role::Cursor));
        }
        true
    }

    /// 後継のプロトコルで次の画面を写させる（写し先が空いていれば）。
    fn capture_ext(&mut self, qh: &QueueHandle<Self>) {
        let Some(layout) = self.ext.layout[Role::Screen as usize] else {
            return;
        };
        let Some((shm, buffer)) = self.buffer(layout, qh) else {
            return;
        };
        let Some(session) = &self.ext.screen else {
            return;
        };
        let frame = session.create_frame(qh, Role::Screen);
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, layout.width as i32, layout.height as i32);
        frame.capture();
        shm.free.store(false, Ordering::Release);
        self.copying = Some((shm, layout));
        self.pending = true;
    }

    /// カーソルの画像を写させる（形が変わるまでコンポジタが待つ）。
    fn capture_cursor(&mut self, qh: &QueueHandle<Self>) {
        let (Some(layout), Some(session)) =
            (self.ext.layout[Role::Cursor as usize], &self.ext.cursor)
        else {
            return;
        };
        if self
            .ext
            .cursor_buffer
            .as_ref()
            .is_none_or(|(_, _, l)| *l != layout)
        {
            if let Some((_, buffer, _)) = self.ext.cursor_buffer.take() {
                buffer.destroy();
            }
            let size = layout.stride as usize * layout.height as usize;
            let (Ok(shm), Some(wl_shm)) = (Shm::new(size, None), &self.shm) else {
                return;
            };
            let pool = wl_shm.create_pool(shm.fd(), shm.len as i32, qh, ());
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
            self.ext.cursor_buffer = Some((shm, buffer, layout));
        }
        let (_, buffer, _) = self.ext.cursor_buffer.as_ref().unwrap();
        let frame = session.create_frame(qh, Role::Cursor);
        frame.attach_buffer(buffer);
        frame.damage_buffer(0, 0, layout.width as i32, layout.height as i32);
        frame.capture();
        self.ext.cursor_pending = true;
    }

    /// 写し終えたカーソルの画像を、知らされたホットスポットとともにカーソルにする。
    fn cursor_done(&mut self) {
        self.ext.cursor_pending = false;
        if let Some((shm, _, layout)) = &self.ext.cursor_buffer {
            let bytes = &shm.bytes()[..layout.stride as usize * layout.height as usize];
            let argb = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from_le_bytes(*p));
            let icon = super::unpremultiply_argb(layout.width, layout.height, argb);
            let visible = icon.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
            self.ext.image = visible.then_some(icon);
        }
        self.publish_cursor();
    }

    fn publish_cursor(&self) {
        let cursor = self
            .ext
            .image
            .as_ref()
            .filter(|_| self.ext.visible)
            .map(|image| Cursor {
                image: image.clone(),
                hotspot: self.ext.hotspot,
            });
        let mut shown = self.shared.cursor.lock().unwrap();
        if shown.as_deref() != cursor.as_ref() {
            *shown = cursor.map(Arc::new);
        }
    }
}

impl Dispatch<copy_session::ExtImageCopyCaptureSessionV1, Role> for State {
    fn event(
        state: &mut Self,
        _: &copy_session::ExtImageCopyCaptureSessionV1,
        event: copy_session::Event,
        role: &Role,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let i = *role as usize;
        match event {
            copy_session::Event::BufferSize { width, height } => {
                state.ext.size[i] = Some((width, height))
            }
            copy_session::Event::ShmFormat { format } if state.ext.format[i].is_none() => {
                logger::trace!("capture format", role = ?role, format = ?format);
                let WEnum::Value(format) = format else { return };
                if pixel_format(format).is_none() {
                    return;
                }
                state.ext.format[i] = Some(format)
            }
            copy_session::Event::Done => {
                logger::debug!(
                    "capture constraints",
                    role = ?role,
                    size = ?state.ext.size[i],
                    format = ?state.ext.format[i]
                );
                if let (Some((width, height)), Some(format)) =
                    (state.ext.size[i], state.ext.format[i])
                {
                    state.ext.layout[i] = Some(Layout {
                        format,
                        width,
                        height,
                        stride: width * 4,
                    });
                }
            }
            copy_session::Event::Stopped if *role == Role::Screen => {
                state.shared.closed.store(true, Ordering::Release);
                state.wake.wake();
            }
            copy_session::Event::Stopped => {
                state.ext.cursor = None;
                state.ext.image = None;
                state.publish_cursor();
            }
            _ => {}
        }
    }
}

impl Dispatch<ext_frame::ExtImageCopyCaptureFrameV1, Role> for State {
    fn event(
        state: &mut Self,
        proxy: &ext_frame::ExtImageCopyCaptureFrameV1,
        event: ext_frame::Event,
        role: &Role,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match (event, role) {
            (
                ext_frame::Event::PresentationTime {
                    tv_sec_hi,
                    tv_sec_lo,
                    tv_nsec,
                },
                Role::Screen,
            ) => {
                state.shown = Some(Duration::new(
                    (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo),
                    tv_nsec,
                ))
            }
            (ext_frame::Event::Ready, Role::Screen) => {
                proxy.destroy();
                state.ext.delivered = true;
                state.deliver();
            }
            (ext_frame::Event::Ready, Role::Cursor) => {
                proxy.destroy();
                state.cursor_done();
            }
            (ext_frame::Event::Failed { reason }, role) => {
                logger::debug!("capture failed", role = ?role, reason = ?reason);
                proxy.destroy();
                // 写し先の条件が変わった（解像度の変更など）。新しい条件が done で届くまで待つ。
                let constraints =
                    reason == WEnum::Value(ext_frame::FailureReason::BufferConstraints);
                if constraints {
                    state.ext.layout[*role as usize] = None;
                    state.ext.format[*role as usize] = None;
                }
                match role {
                    Role::Screen if constraints => {
                        state.pending = false;
                        if let Some((shm, _)) = state.copying.take() {
                            shm.free.store(true, Ordering::Release);
                        }
                    }
                    Role::Screen => state.failed(),
                    Role::Cursor => state.ext.cursor_pending = false,
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<cursor_session::ExtImageCopyCaptureCursorSessionV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &cursor_session::ExtImageCopyCaptureCursorSessionV1,
        event: cursor_session::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        logger::trace!("cursor session", event = ?event);
        match event {
            cursor_session::Event::Enter => state.ext.visible = true,
            cursor_session::Event::Leave => {
                state.ext.visible = false;
                *state.shared.pointer.lock().unwrap() = None;
            }
            cursor_session::Event::Position { x, y } => {
                let (w, h) = *state.shared.size.lock().unwrap();
                *state.shared.pointer.lock().unwrap() = super::normalized(x, y, w, h);
            }
            cursor_session::Event::Hotspot { x, y } => {
                state.ext.hotspot = (x.max(0) as u32, y.max(0) as u32)
            }
            _ => {}
        }
        state.publish_cursor();
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
                // NYAW_SCREENCOPY=wlr なら後継を使わない（wlr-screencopy の検証用）。
                "ext_image_copy_capture_manager_v1" if !force_wlr() => {
                    state.ext.copier = Some(registry.bind(name, 1, qh, ()));
                }
                "ext_output_image_capture_source_manager_v1" if !force_wlr() => {
                    state.ext.sources = Some(registry.bind(name, 1, qh, ()));
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
                state.shown = Some(Duration::new(
                    (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo),
                    tv_nsec,
                ));
                state.deliver();
            }
            frame::Event::Failed => {
                proxy.destroy();
                state.failed();
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
    capture_source::ExtImageCaptureSourceV1,
    capture_sources::ExtOutputImageCaptureSourceManagerV1,
    copier::ExtImageCopyCaptureManagerV1,
    wl_pointer::WlPointer,
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
        if state.ext.screen.is_some()
            && !state.ext.delivered
            && state.manager.is_some()
            && state
                .ext
                .started
                .is_some_and(|t| t.elapsed() > EXT_FIRST_FRAME)
        {
            logger::warn!("ext-image-copy-capture sent no frame; falling back to wlr-screencopy");
            if let Some(session) = state.ext.screen.take() {
                session.destroy();
            }
            if let Some(session) = state.ext.cursor.take() {
                session.destroy();
            }
            state.ext.image = None;
            state.publish_cursor();
            *state.shared.pointer.lock().unwrap() = None;
            // 待っていた写し先は、届かないまま破棄される。
            if let Some((shm, _)) = state.copying.take() {
                shm.free.store(true, Ordering::Release);
            }
            state.pending = false;
        }
        if state.ext.screen.is_some() {
            if !state.pending && room {
                state.capture_ext(&qh);
            }
            if !state.ext.cursor_pending {
                state.capture_cursor(&qh);
            }
        } else if !state.pending
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
    if let Some((_, buffer, _)) = state.ext.cursor_buffer.take() {
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
            shown: None,
            ext: Ext::default(),
        };
        queue.roundtrip(&mut state)?;
        let Some(output) = state.output.clone() else {
            return Err(Error(crate::failure::NO_PIXELS.into()));
        };
        // 後継の ext-image-copy-capture を主に使い、無ければ wlr-screencopy に戻す。
        if state.start_ext(&qh) {
            logger::info!("capturing with ext-image-copy-capture");
        } else if state.manager.is_some() {
            logger::info!("capturing with wlr-screencopy (no cursor image)");
        } else {
            return Err(Error(crate::failure::UNSUPPORTED.into()));
        }
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

    fn cursor(&mut self) -> Option<Arc<Cursor>> {
        self.shared.cursor.lock().unwrap().clone()
    }

    fn pointer(&mut self) -> Option<(f32, f32)> {
        *self.shared.pointer.lock().unwrap()
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
        // 大きさが分からないうちは 10000 画素とみなす（出力に対する割合で解釈される）。
        let (w, h) = match self.size {
            (0, _) | (_, 0) => (10_000, 10_000),
            size => size,
        };
        // 画素の中心を指す（範囲を 2 倍にして 2x+1 を送る）。左上の角を送ると、出力の向きによっては隣の画素に入る。
        let px = super::pixel(x, w) as u32 * 2 + 1;
        let py = super::pixel(y, h) as u32 * 2 + 1;
        self.pointer
            .motion_absolute(self.time(), px, py, w * 2, h * 2);
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
