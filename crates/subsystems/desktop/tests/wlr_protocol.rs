//! wlroots の取り込み（wlr-screencopy）と入力（wlr の仮想ポインタ）を、テスト内の最小限のコンポジタで検証する。
//! 実際の wlroots 系のコンポジタが無くても、クライアントとしての手順（バッファの用意、写させる、仮想ポインタの座標）を確かめられる。
#![cfg(target_os = "linux")]

use std::{
    os::fd::OwnedFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use desktop::{Button, SourceKind, Sources, Status, Stream, Touch, TouchPhase};
use rustix::mm;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::server::{
    zwp_virtual_keyboard_manager_v1 as keyboards, zwp_virtual_keyboard_v1 as virtual_keyboard,
};
use wayland_protocols_wlr::{
    screencopy::v1::server::{
        zwlr_screencopy_frame_v1 as frame, zwlr_screencopy_manager_v1 as manager,
    },
    virtual_pointer::v1::server::{
        zwlr_virtual_pointer_manager_v1 as pointers, zwlr_virtual_pointer_v1 as pointer,
    },
};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, ListeningSocket, New,
    Resource,
    backend::{ClientData, ClientId, DisconnectReason},
    protocol::{wl_buffer, wl_keyboard, wl_output, wl_seat, wl_shm, wl_shm_pool},
};

/// コンポジタの配列として送る文字列（仮想キーボードがキーを送るときに、そのまま使い戻されることを確かめる）。
const SYSTEM_KEYMAP: &str = "xkb_keymap { system };";

/// 仮想キーボードから受け取った配列の要約（コンポジタの配列か、文字ごとの配列の keysym 名の並び）。
fn keymap_summary(text: &str) -> String {
    if text == SYSTEM_KEYMAP {
        return "keymap system".into();
    }
    let symbols: Vec<&str> = text
        .split("{[ ")
        .skip(1)
        .filter_map(|s| s.split(' ').next())
        .collect();
    format!("keymap {}", symbols.join(" "))
}

const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;

/// 画面の (x, y) の画素（B, G, R, 未使用）。`n` は何枚目の画面か。
fn pixel(x: u32, y: u32, n: u8) -> [u8; 4] {
    [x as u8 * 3, y as u8 * 7, n, 0]
}

struct Mapping {
    ptr: *mut u8,
    len: usize,
    _fd: OwnedFd,
}
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}
impl Drop for Mapping {
    fn drop(&mut self) {
        let _ = unsafe { mm::munmap(self.ptr.cast(), self.len) };
    }
}

struct Buffer {
    pool: Arc<Mapping>,
    offset: usize,
    stride: usize,
}

#[derive(Default)]
struct State {
    /// 受け取った仮想ポインタの操作。
    events: Arc<Mutex<Vec<String>>>,
    frames: u8,
}

struct NoData;
impl ClientData for NoData {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

impl GlobalDispatch<wl_output::WlOutput, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_output::WlOutput>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let output = init.init(resource, ());
        output.geometry(
            0,
            0,
            300,
            150,
            wl_output::Subpixel::Unknown,
            "Fake".into(),
            "WLR TEST".into(),
            wl_output::Transform::Normal,
        );
        output.mode(
            wl_output::Mode::Current,
            WIDTH as i32,
            HEIGHT as i32,
            60_000,
        );
        if output.version() >= 4 {
            output.name("FAKE-1".into());
        }
        if output.version() >= 2 {
            output.done();
        }
    }
}

impl GlobalDispatch<wl_shm::WlShm, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_shm::WlShm>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let shm = init.init(resource, ());
        shm.format(wl_shm::Format::Xrgb8888);
        shm.format(wl_shm::Format::Argb8888);
    }
}

impl GlobalDispatch<wl_seat::WlSeat, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_seat::WlSeat>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ())
            .capabilities(wl_seat::Capability::Pointer);
    }
}

impl GlobalDispatch<manager::ZwlrScreencopyManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<manager::ZwlrScreencopyManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl GlobalDispatch<pointers::ZwlrVirtualPointerManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<pointers::ZwlrVirtualPointerManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<wl_shm::WlShm, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_shm::WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm::Request::CreatePool { id, fd, size } = request {
            let len = size as usize;
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
            .expect("map the client's pool") as *mut u8;
            init.init(id, Arc::new(Mapping { ptr, len, _fd: fd }));
        }
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, Arc<Mapping>> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_shm_pool::WlShmPool,
        request: wl_shm_pool::Request,
        pool: &Arc<Mapping>,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm_pool::Request::CreateBuffer {
            id, offset, stride, ..
        } = request
        {
            init.init(
                id,
                Buffer {
                    pool: pool.clone(),
                    offset: offset as usize,
                    stride: stride as usize,
                },
            );
        }
    }
}

impl Dispatch<manager::ZwlrScreencopyManagerV1, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        resource: &manager::ZwlrScreencopyManagerV1,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let manager::Request::CaptureOutput {
            frame,
            overlay_cursor,
            ..
        } = request
        {
            assert_eq!(overlay_cursor, 0, "the cursor stays out of the picture");
            let frame = init.init(frame, ());
            frame.buffer(wl_shm::Format::Xrgb8888, WIDTH, HEIGHT, WIDTH * 4);
            if resource.version() >= 3 {
                frame.buffer_done();
            }
        }
    }
}

impl Dispatch<frame::ZwlrScreencopyFrameV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &frame::ZwlrScreencopyFrameV1,
        request: frame::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let buffer = match request {
            frame::Request::Copy { buffer } | frame::Request::CopyWithDamage { buffer } => buffer,
            _ => return,
        };
        let b = buffer.data::<Buffer>().unwrap();
        state.frames = state.frames.wrapping_add(1);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = b.offset + y as usize * b.stride + x as usize * 4;
                assert!(i + 4 <= b.pool.len);
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        pixel(x, y, state.frames).as_ptr(),
                        b.pool.ptr.add(i),
                        4,
                    )
                };
            }
        }
        resource.flags(frame::Flags::empty());
        if resource.version() >= 2 {
            resource.damage(0, 0, WIDTH, HEIGHT);
        }
        let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        resource.ready(0, now.tv_sec as u32, now.tv_nsec as u32);
    }
}

impl Dispatch<pointers::ZwlrVirtualPointerManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &pointers::ZwlrVirtualPointerManagerV1,
        request: pointers::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        match request {
            pointers::Request::CreateVirtualPointerWithOutput { id, output, .. } => {
                let bound = output.is_some();
                state
                    .events
                    .lock()
                    .unwrap()
                    .push(format!("pointer on output {bound}"));
                init.init(id, ());
            }
            pointers::Request::CreateVirtualPointer { id, .. } => {
                init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<pointer::ZwlrVirtualPointerV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &pointer::ZwlrVirtualPointerV1,
        request: pointer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let event = match request {
            pointer::Request::MotionAbsolute {
                x,
                y,
                x_extent,
                y_extent,
                ..
            } => format!("motion {x} {y} of {x_extent}x{y_extent}"),
            pointer::Request::Button { button, state, .. } => {
                format!("button {button:#x} {:?}", state.into_result().unwrap())
            }
            pointer::Request::AxisDiscrete { axis, discrete, .. } => {
                format!("axis {:?} {discrete}", axis.into_result().unwrap())
            }
            _ => return,
        };
        state.events.lock().unwrap().push(event);
    }
}

/// 要求を受け取るだけで応答の要らないオブジェクト。
macro_rules! accept_requests {
    ($($resource:ty),*) => {$(
        impl Dispatch<$resource, ()> for State {
            fn request(_: &mut Self, _: &Client, _: &$resource, _: <$resource as Resource>::Request, _: &(), _: &DisplayHandle, _: &mut DataInit<'_, Self>) {}
        }
    )*};
}
accept_requests!(wl_output::WlOutput, wl_keyboard::WlKeyboard);

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_seat::WlSeat,
        request: wl_seat::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_seat::Request::GetKeyboard { id } = request {
            let keyboard = init.init(id, ());
            let fd = rustix::fs::memfd_create(c"keymap", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
            let mut text = SYSTEM_KEYMAP.as_bytes().to_vec();
            text.push(0);
            rustix::io::write(&fd, &text).unwrap();
            keyboard.keymap(
                wl_keyboard::KeymapFormat::XkbV1,
                std::os::fd::AsFd::as_fd(&fd),
                text.len() as u32,
            );
        }
    }
}

impl GlobalDispatch<keyboards::ZwpVirtualKeyboardManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<keyboards::ZwpVirtualKeyboardManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<keyboards::ZwpVirtualKeyboardManagerV1, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &keyboards::ZwpVirtualKeyboardManagerV1,
        request: keyboards::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let keyboards::Request::CreateVirtualKeyboard { id, .. } = request {
            init.init(id, ());
        }
    }
}

impl Dispatch<virtual_keyboard::ZwpVirtualKeyboardV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &virtual_keyboard::ZwpVirtualKeyboardV1,
        request: virtual_keyboard::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let event = match request {
            virtual_keyboard::Request::Keymap { fd, size, .. } => {
                let len = size as usize;
                let ptr = unsafe {
                    mm::mmap(
                        std::ptr::null_mut(),
                        len,
                        mm::ProtFlags::READ,
                        mm::MapFlags::PRIVATE,
                        &fd,
                        0,
                    )
                }
                .unwrap();
                let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
                let text =
                    String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap()).into_owned();
                unsafe { mm::munmap(ptr, len).unwrap() };
                keymap_summary(&text)
            }
            virtual_keyboard::Request::Key { key, state, .. } => format!("key {key} {state}"),
            virtual_keyboard::Request::Modifiers {
                mods_depressed,
                mods_locked,
                ..
            } => format!("mods {mods_depressed} {mods_locked}"),
            _ => return,
        };
        state.events.lock().unwrap().push(event);
    }
}

impl Dispatch<wl_buffer::WlBuffer, Buffer> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Request,
        _: &Buffer,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

/// 別スレッドで動く最小限のコンポジタ。
struct Compositor {
    name: String,
    events: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Compositor {
    fn start() -> Option<Self> {
        let socket = ListeningSocket::bind_auto("nyaw-wlr-test", 0..32)
            .inspect_err(|e| eprintln!("cannot open a Wayland socket: {e}"))
            .ok()?;
        let name = socket.socket_name()?.to_string_lossy().into_owned();
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, started) = mpsc::channel();
        let thread = {
            let (events, stop) = (events.clone(), stop.clone());
            thread::spawn(move || {
                let mut display: Display<State> = Display::new().unwrap();
                let handle = display.handle();
                handle.create_global::<State, wl_output::WlOutput, ()>(4, ());
                handle.create_global::<State, wl_shm::WlShm, ()>(1, ());
                handle.create_global::<State, wl_seat::WlSeat, ()>(7, ());
                handle.create_global::<State, manager::ZwlrScreencopyManagerV1, ()>(3, ());
                handle.create_global::<State, pointers::ZwlrVirtualPointerManagerV1, ()>(2, ());
                handle.create_global::<State, keyboards::ZwpVirtualKeyboardManagerV1, ()>(1, ());
                let mut state = State { events, frames: 0 };
                ready.send(()).unwrap();
                while !stop.load(Ordering::Acquire) {
                    while let Ok(Some(client)) = socket.accept() {
                        let _ = display.handle().insert_client(client, Arc::new(NoData));
                    }
                    let _ = display.dispatch_clients(&mut state);
                    let _ = display.flush_clients();
                    thread::sleep(Duration::from_millis(2));
                }
            })
        };
        started.recv().unwrap();
        Some(Self {
            name,
            events,
            stop,
            thread: Some(thread),
        })
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn wlroots_streams_and_points_through_screencopy_and_a_virtual_pointer() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    // SAFETY: このファイルのテストはこれだけで、取り込みのスレッドはこの後に起動する。
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &compositor.name);
        std::env::remove_var("NYAW_CAPTURE");
    }
    let pending = Sources::refresh().unwrap();
    let screens = until("the screen list", || pending.poll()).unwrap();
    assert_eq!(screens.len(), 1, "{screens:?}");
    assert_eq!(
        (screens[0].name.as_str(), screens[0].kind),
        ("WLR TEST", SourceKind::Screen)
    );

    for import in [
        None,
        Some(vr::ImportFormats {
            host_alignment: Some(4096),
            ..Default::default()
        }),
    ] {
        let lent = import.is_some();
        let stream = Stream::start_with(screens[0].clone(), 60, import).unwrap();
        let frame = until("a frame", || {
            let update = stream.poll()?;
            if let Status::Failed(e) = &update.status {
                panic!("capture failed: {e}");
            }
            update.frame
        });
        assert_eq!((frame.width, frame.height), (WIDTH, HEIGHT));
        // 取り込めるなら共有メモリのまま渡し、取り込めなければ RGBA（不透明）に並べ替える。
        let at = |x: u32, y: u32| -> [u8; 4] {
            match &frame.external {
                Some(image) => {
                    let (bytes, stride) = image.host_bytes().unwrap();
                    let i = y as usize * stride + x as usize * 4;
                    let p = &bytes[i..i + 4];
                    [p[2], p[1], p[0], 255]
                }
                None => {
                    let i = ((y * WIDTH + x) * 4) as usize;
                    frame.rgba[i..i + 4].try_into().unwrap()
                }
            }
        };
        assert_eq!(frame.external.is_some(), lent);
        let [b, g, _, _] = pixel(5, 9, 0);
        let [r, gg, bb, a] = at(5, 9);
        assert_eq!((gg, bb, a), (g, b, 255));
        assert!(r > 0, "red counts the frames");
        drop(frame);

        let touch = |phase, x| Touch {
            id: 0,
            phase,
            x,
            y: 0.5,
        };
        stream.touch(touch(TouchPhase::Down, 0.0)).unwrap();
        stream
            .button(touch(TouchPhase::Up, 1.0), Button::Left)
            .unwrap();
        stream.scroll(0.5, 0.5, 0.0, 3.0).unwrap();
        let wanted = [
            "pointer on output true",
            "motion 0 16 of 64x32",
            "button 0x110 Pressed",
            "motion 63 16 of 64x32",
            "button 0x110 Released",
            "motion 32 16 of 64x32",
            "axis VerticalScroll -1",
        ];
        until("the pointer", || {
            let events = compositor.events();
            (events.len() >= wanted.len()).then_some(())
        });
        let events = compositor.events();
        assert_eq!(
            events.iter().map(String::as_str).collect::<Vec<_>>(),
            wanted,
            "{events:?}"
        );
        compositor.events.lock().unwrap().clear();
    }

    // OS キーボード: キーはコンポジタの配列で送り、修飾キーは修飾子も送る。文字列は文字ごとの配列で送ってから戻す。
    use os::keyboard::{Output, chord};
    let keyboard = desktop::os_keyboard();
    let settings = until("the keyboard", || {
        let s = keyboard.settings();
        (s.foreground != 0).then_some(s)
    });
    assert!(settings.error.is_none(), "{:?}", settings.error);
    let expect = |wanted: &[&str]| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while compositor.events().len() < wanted.len() {
            assert!(
                Instant::now() < deadline,
                "got {:?}, want {wanted:?}",
                compositor.events()
            );
            thread::sleep(Duration::from_millis(5));
        }
        let events = compositor.events();
        assert_eq!(
            events.iter().map(String::as_str).collect::<Vec<_>>(),
            wanted,
            "{events:?}"
        );
        compositor.events.lock().unwrap().clear();
    };
    expect(&["keymap system"]);
    keyboard
        .send(settings.foreground, Output::Keys(chord(&[42], 30)))
        .unwrap();
    expect(&[
        "key 42 1", "mods 1 0", "key 30 1", "key 30 0", "key 42 0", "mods 0 0",
    ]);
    keyboard
        .send(settings.foreground, Output::Text("aあa".into()))
        .unwrap();
    expect(&[
        "keymap U0061 U3042",
        "mods 0 0",
        "key 1 1",
        "key 1 0",
        "key 2 1",
        "key 2 0",
        "key 1 1",
        "key 1 0",
        "keymap system",
        "mods 0 0",
    ]);
    assert!(keyboard.settings().error.is_none());
}
