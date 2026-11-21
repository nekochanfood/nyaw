//! 実際の Wayland コンポジタ（wlroots 系、niri など）での取り込み・ポインタ・OS キーボードの検証。
//! 利用者のデスクトップを操作しないよう、`NYAW_TEST_WAYLAND` に指定したコンポジタ（入れ子で起動したものなど）だけを使う。
//! `NYAW_TEST_WAYLAND=wayland-2 cargo test -p desktop --test wayland_compositor -- --ignored`
#![cfg(target_os = "linux")]

use std::{
    io::Write as _,
    os::fd::AsFd,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use desktop::{Button, Sources, Status, Stream, Touch, TouchPhase};
use os::keyboard::{Output, chord};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use xkbcommon::xkb;

/// テスト用ウィンドウの (x, y) の画素の色（R, G, B）。色から位置を読み戻せる（青の上位 2 ビットが 1 の画素はウィンドウ）。
fn encode(x: u32, y: u32) -> [u8; 3] {
    [
        x as u8,
        y as u8,
        0xc0 | ((x >> 8) as u8 & 7) << 3 | ((y >> 8) as u8 & 7),
    ]
}

fn decode([r, g, b]: [u8; 3]) -> Option<(u32, u32)> {
    (b & 0xc0 == 0xc0).then(|| {
        (
            u32::from(r) | (u32::from(b >> 3) & 7) << 8,
            u32::from(g) | (u32::from(b) & 7) << 8,
        )
    })
}

/// テスト用ウィンドウが受け取ったもの。
#[derive(Default)]
struct Seen {
    pointer: Option<(f64, f64)>,
    buttons: Vec<(u32, bool)>,
    typed: String,
}

struct Client {
    seen: Arc<Mutex<Seen>>,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm: Option<xdg_wm_base::XdgWmBase>,
    seat: Option<wl_seat::WlSeat>,
    surface: Option<wl_surface::WlSurface>,
    xkb: Option<xkb::State>,
    size: (i32, i32),
}

impl Dispatch<wl_registry::WlRegistry, ()> for Client {
    fn event(
        c: &mut Self,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_compositor" => c.compositor = Some(r.bind(name, version.min(4), qh, ())),
                "wl_shm" => c.shm = Some(r.bind(name, 1, qh, ())),
                "xdg_wm_base" => c.wm = Some(r.bind(name, 1, qh, ())),
                "wl_seat" if c.seat.is_none() => {
                    c.seat = Some(r.bind(name, version.min(5), qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        e: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = e {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for Client {
    fn event(
        c: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        e: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Configure { width, height, .. } = e
            && width > 0
            && height > 0
        {
            c.size = (width, height);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for Client {
    fn event(
        c: &mut Self,
        s: &xdg_surface::XdgSurface,
        e: xdg_surface::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let xdg_surface::Event::Configure { serial } = e else {
            return;
        };
        s.ack_configure(serial);
        let (w, h) = c.size;
        let (w, h) = if w > 0 { (w, h) } else { (400, 300) };
        let len = (w * h * 4) as usize;
        let fd = rustix::fs::memfd_create(c"client", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        let mut file = std::fs::File::from(fd);
        let mut pixels = Vec::with_capacity(len);
        for y in 0..h as u32 {
            for x in 0..w as u32 {
                let [r, g, b] = encode(x, y);
                pixels.extend([b, g, r, 0xff]);
            }
        }
        file.write_all(&pixels).unwrap();
        let pool = c
            .shm
            .as_ref()
            .unwrap()
            .create_pool(file.as_fd(), len as i32, qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Xrgb8888, qh, ());
        pool.destroy();
        let surface = c.surface.as_ref().unwrap();
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, w, h);
        surface.commit();
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Client {
    fn event(
        _: &mut Self,
        seat: &wl_seat::WlSeat,
        e: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = e
        {
            if caps.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
            }
            if caps.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Client {
    fn event(
        c: &mut Self,
        _: &wl_pointer::WlPointer,
        e: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut seen = c.seen.lock().unwrap();
        match e {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            }
            | wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => seen.pointer = Some((surface_x, surface_y)),
            wl_pointer::Event::Button { button, state, .. } => seen.buttons.push((
                button,
                state == WEnum::Value(wl_pointer::ButtonState::Pressed),
            )),
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        c: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        e: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            wl_keyboard::Event::Keymap { fd, size, .. } => {
                let mut text = vec![0u8; size as usize];
                let file = std::fs::File::from(fd);
                std::os::unix::fs::FileExt::read_exact_at(&file, &mut text, 0).unwrap();
                let text = String::from_utf8_lossy(text.split(|&b| b == 0).next().unwrap());
                let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                c.xkb = xkb::Keymap::new_from_string(
                    &context,
                    text.into_owned(),
                    xkb::KEYMAP_FORMAT_TEXT_V1,
                    xkb::KEYMAP_COMPILE_NO_FLAGS,
                )
                .map(|k| xkb::State::new(&k));
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(state) = &mut c.xkb {
                    state.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                if let Some(state) = &c.xkb {
                    let text = state.key_get_utf8(xkb::Keycode::new(key + 8));
                    c.seen.lock().unwrap().typed.push_str(&text);
                }
            }
            _ => {}
        }
    }
}

macro_rules! no_events {
    ($($p:ty),*) => {$(
        impl Dispatch<$p, ()> for Client {
            fn event(_: &mut Self, _: &$p, _: <$p as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
no_events!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    wl_surface::WlSurface
);

/// 既知の色のウィンドウを開き、受け取ったものを記録し続ける。
fn open_window(display: &str) -> Arc<Mutex<Seen>> {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let record = seen.clone();
    let display = display.to_owned();
    thread::spawn(move || {
        // SAFETY: 接続はこのスレッドだけが使う（環境変数は読まず、ソケット名を直接渡す）。
        let path = std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap()).join(&display);
        let stream = std::os::unix::net::UnixStream::connect(path).unwrap();
        let conn = Connection::from_socket(stream).unwrap();
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut client = Client {
            seen: record,
            compositor: None,
            shm: None,
            wm: None,
            seat: None,
            surface: None,
            xkb: None,
            size: (0, 0),
        };
        queue.roundtrip(&mut client).unwrap();
        let surface = client.compositor.as_ref().unwrap().create_surface(&qh, ());
        let xdg = client
            .wm
            .as_ref()
            .unwrap()
            .get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        toplevel.set_title("nyaw test".into());
        surface.commit();
        client.surface = Some(surface);
        loop {
            if queue.blocking_dispatch(&mut client).is_err() {
                break;
            }
        }
    });
    seen
}

fn until<T>(what: &str, timeout: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "needs a Wayland compositor given in NYAW_TEST_WAYLAND (a nested niri or sway)"]
fn a_wayland_compositor_streams_points_and_types() {
    let Ok(display) = std::env::var("NYAW_TEST_WAYLAND") else {
        panic!("set NYAW_TEST_WAYLAND to the socket of a compositor to test");
    };
    // SAFETY: 取り込みのスレッドを起動する前に一度だけ書き換える。
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &display);
        std::env::remove_var("NYAW_CAPTURE");
    }
    let pending = Sources::refresh().unwrap();
    let screens = until("the screens", Duration::from_secs(5), || pending.poll()).unwrap();
    eprintln!(
        "screens: {:?}",
        screens.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    let stream = Stream::start_with(
        screens[0].clone(),
        60,
        Some(vr::ImportFormats {
            host_alignment: Some(4096),
            ..Default::default()
        }),
    )
    .unwrap();
    let seen = open_window(&display);
    // 取り込んだ画面で、テスト用ウィンドウの中ほどの画素を選び、その色からウィンドウ内の位置を読む。
    let (size, points) = until("the window on screen", Duration::from_secs(10), || {
        let update = stream.poll()?;
        if let Status::Failed(e) = &update.status {
            panic!("capture failed: {e}");
        }
        let frame = update.frame?;
        let (bytes, stride) = frame.external.as_ref()?.host_bytes()?;
        let at = |x: u32, y: u32| {
            let i = y as usize * stride + x as usize * 4;
            [bytes[i + 2], bytes[i + 1], bytes[i]]
        };
        let hits: Vec<(u32, u32)> = (0..frame.height)
            .step_by(4)
            .flat_map(|y| (0..frame.width).step_by(4).map(move |x| (x, y)))
            .filter(|&(x, y)| decode(at(x, y)).is_some())
            .collect();
        if hits.len() < 100 {
            return None;
        }
        // 上寄りと下寄りの点（上下が反転していれば、どちらかで大きくずれる）。
        let (y_min, y_max) = (
            hits.iter().map(|h| h.1).min()?,
            hits.iter().map(|h| h.1).max()?,
        );
        let x_mid = hits.iter().map(|h| u64::from(h.0)).sum::<u64>() / hits.len() as u64;
        let points: Vec<((u32, u32), (u32, u32))> = [4, 1, 3]
            .into_iter()
            .map(|q| {
                let target = (x_mid as u32 + q, y_min + (y_max - y_min) * q / 5 + 2);
                decode(at(target.0, target.1)).map(|local| (target, local))
            })
            .collect::<Option<_>>()?;
        Some(((frame.width, frame.height), points))
    });

    // マウス: 画面で見えている点を指すと、ウィンドウはその点の位置でポインタを受け取る（出力の向きによらない）。
    let (w, h) = (size.0 as f32 - 1.0, size.1 as f32 - 1.0);
    let mut at = Touch {
        id: 0,
        phase: TouchPhase::Move,
        x: 0.0,
        y: 0.0,
    };
    for (target, local) in points {
        at.x = target.0 as f32 / w;
        at.y = target.1 as f32 / h;
        stream.touch(at).unwrap();
        // 開いた時点のポインタの位置も届くため、送った点に来るまで待つ。
        let near = |p: (f64, f64)| (p.0.floor(), p.1.floor()) == (local.0 as f64, local.1 as f64);
        let deadline = Instant::now() + Duration::from_secs(5);
        let pointer = loop {
            let p = seen.lock().unwrap().pointer;
            if let Some(p) = p.filter(|p| near(*p)) {
                break p;
            }
            assert!(
                Instant::now() < deadline,
                "pointed at {target:?} showing the window's {local:?}, but the window has the pointer at {p:?}"
            );
            thread::sleep(Duration::from_millis(10));
        };
        eprintln!(
            "screen {target:?} shows the window's {local:?}; pointing there, the window got {pointer:?}"
        );
    }
    let before = seen.lock().unwrap().pointer;
    stream
        .touch(Touch {
            x: at.x + 10.0 / w,
            ..at
        })
        .unwrap();
    let moved = until("the pointer to move", Duration::from_secs(5), || {
        let p = seen.lock().unwrap().pointer;
        (p != before).then_some(p)
    })
    .unwrap();
    assert_eq!(
        (moved.0 - before.unwrap().0).abs().round(),
        10.0,
        "moves by whole pixels"
    );
    stream
        .button(
            Touch {
                phase: TouchPhase::Down,
                ..at
            },
            Button::Right,
        )
        .unwrap();
    stream
        .button(
            Touch {
                phase: TouchPhase::Up,
                ..at
            },
            Button::Right,
        )
        .unwrap();
    until("the click", Duration::from_secs(5), || {
        (seen.lock().unwrap().buttons == [(0x111, true), (0x111, false)]).then_some(())
    });

    // キーボード: フォーカスしているテスト用ウィンドウへ、キーと、配列に無い文字を含む文字列を送る。
    let keyboard = desktop::os_keyboard();
    let settings = until("the keyboard", Duration::from_secs(5), || {
        let s = keyboard.settings();
        (s.foreground != 0 || s.error.is_some()).then_some(s)
    });
    assert!(settings.error.is_none(), "{:?}", settings.error);
    keyboard
        .send(settings.foreground, Output::Keys(chord(&[42], 30)))
        .unwrap();
    until("Shift+A", Duration::from_secs(5), || {
        (seen.lock().unwrap().typed == "A").then_some(())
    });
    keyboard
        .send(settings.foreground, Output::Text("hé語!".into()))
        .unwrap();
    until("the text", Duration::from_secs(5), || {
        (seen.lock().unwrap().typed == "Ahé語!").then_some(())
    });
    // 文字列の後は元の配列に戻り、キーはまた配列どおりに入力される。
    keyboard
        .send(settings.foreground, Output::Keys(chord(&[], 48)))
        .unwrap();
    until("b after the text", Duration::from_secs(5), || {
        (seen.lock().unwrap().typed == "Ahé語!b").then_some(())
    });
    eprintln!("typed: {}", seen.lock().unwrap().typed);
}
