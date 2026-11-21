//! Linux の実機の取り込みと入力注入（表示サーバーが要る。`cargo test -p desktop --test linux_capture -- --ignored --test-threads=1`）。
//! X11 は rootful の Xwayland を別に起動して検証する（デスクトップに小さなウィンドウが一時的に開く）。
//! portal は確認画面で画面の共有を許可する操作が要る（復元のトークンが保存されていれば出ない）。
#![cfg(target_os = "linux")]

use std::{
    process::{Child, Command},
    sync::Arc,
    time::{Duration, Instant},
};

use desktop::{Frame, SourceKind, Sources, Status, Stream, Touch, TouchPhase};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            ChangeWindowAttributesAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask,
            InputFocus, Rectangle, WindowClass,
        },
    },
};

fn until<T>(what: &str, timeout: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn sources() -> Vec<desktop::Source> {
    let pending = Sources::refresh().unwrap();
    until("sources", Duration::from_secs(10), || pending.poll()).unwrap()
}

fn frame(stream: &Stream, timeout: Duration) -> Arc<Frame> {
    until("a frame", timeout, || {
        let update = stream.poll()?;
        if let Status::Failed(e) = &update.status {
            panic!("capture failed: {e}");
        }
        update.frame
    })
}

/// RGBA の画素（取り込めた画像は ExternalImage の共有メモリから読む）。
fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
    if let Some(image) = &frame.external {
        let (bytes, stride) = image.host_bytes().expect("shared memory image");
        let i = y as usize * stride + x as usize * 4;
        let p = &bytes[i..i + 4];
        let [r, g, b] = if image.format.bgr() {
            [p[2], p[1], p[0]]
        } else {
            [p[0], p[1], p[2]]
        };
        return [r, g, b, if image.format.opaque() { 255 } else { p[3] }];
    }
    let i = ((y * frame.width + x) * 4) as usize;
    frame.rgba[i..i + 4].try_into().unwrap()
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// X11: 画面を XSHM で取り込み（RGBA と、ランタイムへ渡す共有メモリの両方）、XTest のクリックがウィンドウへ届く。
#[test]
#[ignore = "starts a rootful Xwayland on the desktop"]
fn x11_captures_the_screen_and_clicks_through_xtest() {
    let display = ":57";
    let _server = Server(
        Command::new("Xwayland")
            .args([display, "-geometry", "320x240", "-noreset"])
            .spawn()
            .expect("Xwayland"),
    );
    let (conn, screen) = until("Xwayland", Duration::from_secs(10), || {
        x11rb::connect(Some(display)).ok()
    });
    // SAFETY: このテストは単独で実行する（--test-threads=1）。
    unsafe {
        std::env::set_var("DISPLAY", display);
        std::env::set_var("NYAW_CAPTURE", "x11");
    }
    let root = conn.setup().roots[screen].root;
    let window = conn.generate_id().unwrap();
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        40,
        30,
        100,
        80,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new()
            .background_pixel(0x3366cc)
            .override_redirect(1)
            .event_mask(EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE),
    )
    .unwrap();
    conn.map_window(window).unwrap();
    conn.flush().unwrap();

    let screens = sources();
    assert_eq!(screens.len(), 1, "{screens:?}");
    assert_eq!(screens[0].kind, SourceKind::Screen);
    for import in [
        None,
        Some(vr::ImportFormats {
            host_alignment: Some(4096),
            ..Default::default()
        }),
    ] {
        let lent = import.is_some();
        let stream = Stream::start_with(screens[0].clone(), 60, import).unwrap();
        let shot = until("the window on screen", Duration::from_secs(5), || {
            let f = frame(&stream, Duration::from_secs(5));
            (pixel(&f, 90, 70) == [0x33, 0x66, 0xcc, 255]).then_some(f)
        });
        assert_eq!((shot.width, shot.height), (320, 240));
        assert_eq!(shot.external.is_some(), lent);
        assert!(shot.rgba.is_empty() == lent);
        drop(shot);

        // ウィンドウの中央（90, 70）をクリックする。
        let at = |phase| Touch {
            id: 0,
            phase,
            x: 90.0 / 319.0,
            y: 70.0 / 239.0,
        };
        stream.touch(at(TouchPhase::Down)).unwrap();
        stream.touch(at(TouchPhase::Up)).unwrap();
        let press = until("a click", Duration::from_secs(5), || {
            match conn.poll_for_event().unwrap()? {
                Event::ButtonPress(e) => Some(e),
                _ => None,
            }
        });
        assert_eq!((press.detail, press.event_x, press.event_y), (1, 50, 40));
    }

    // ポインタの位置: XTest で動かした画素に X サーバーのポインタがあり、取り込みもその位置を知らせる。
    let stream = Stream::start(screens[0].clone()).unwrap();
    frame(&stream, Duration::from_secs(5));
    for (x, y) in [(0.25, 0.75), (0.0, 0.0), (1.0, 1.0), (0.5, 0.5)] {
        stream
            .touch(Touch {
                id: 0,
                phase: TouchPhase::Move,
                x,
                y,
            })
            .unwrap();
        let want = ((319.0 * x).round() as i16, (239.0 * y).round() as i16);
        until("the X pointer", Duration::from_secs(5), || {
            let p = conn.query_pointer(root).unwrap().reply().unwrap();
            ((p.root_x, p.root_y) == want).then_some(())
        });
        let reported = until("the reported pointer", Duration::from_secs(5), || {
            stream.pointer().filter(|p| {
                (p.0 * 319.0).round() as i16 == want.0 && (p.1 * 239.0).round() as i16 == want.1
            })
        });
        assert!((reported.0 - x).abs() < 0.01 && (reported.1 - y).abs() < 0.01);
    }

    // カーソルの画像: 左半分が赤く右半分が透明な 16×16（ホットスポット 3, 5）をウィンドウに設定し、ポインタを
    // ウィンドウに入れると、その画像とホットスポットが XFixes から読める。
    let (source, mask) = (conn.generate_id().unwrap(), conn.generate_id().unwrap());
    conn.create_pixmap(1, source, root, 16, 16).unwrap();
    conn.create_pixmap(1, mask, root, 16, 16).unwrap();
    let fill = |pixmap, value, width| {
        let gc = conn.generate_id().unwrap();
        conn.create_gc(gc, pixmap, &CreateGCAux::new().foreground(value))
            .unwrap();
        conn.poly_fill_rectangle(
            pixmap,
            gc,
            &[Rectangle {
                x: 0,
                y: 0,
                width,
                height: 16,
            }],
        )
        .unwrap();
        conn.free_gc(gc).unwrap();
    };
    fill(source, 1, 16);
    fill(mask, 0, 16);
    fill(mask, 1, 8);
    let cursor = conn.generate_id().unwrap();
    conn.create_cursor(cursor, source, mask, 65535, 0, 0, 0, 0, 0, 3, 5)
        .unwrap();
    conn.change_window_attributes(window, &ChangeWindowAttributesAux::new().cursor(cursor))
        .unwrap();
    conn.flush().unwrap();
    let outside = until("the root cursor", Duration::from_secs(5), || {
        stream.cursor()
    });
    stream
        .touch(Touch {
            id: 0,
            phase: TouchPhase::Move,
            x: 90.0 / 319.0,
            y: 70.0 / 239.0,
        })
        .unwrap();
    let shown = until("the window's cursor", Duration::from_secs(5), || {
        stream.cursor().filter(|c| !Arc::ptr_eq(c, &outside))
    });
    assert_eq!((shown.image.width, shown.image.height), (16, 16));
    assert_eq!(shown.hotspot, (3, 5));
    let px = |x: usize, y: usize| -> [u8; 4] {
        let i = (y * 16 + x) * 4;
        shown.image.rgba[i..i + 4].try_into().unwrap()
    };
    assert_eq!(px(2, 9), [255, 0, 0, 255]);
    assert_eq!(px(12, 9), [0, 0, 0, 0]);
}

/// portal: 確認画面で選んだ画面を PipeWire で受け取り、RemoteDesktop でポインタを動かす。
#[test]
#[ignore = "needs a Wayland session with xdg-desktop-portal, and may ask for permission"]
fn portal_streams_a_screen() {
    // SAFETY: このテストは単独で実行する（--test-threads=1）。
    unsafe { std::env::set_var("NYAW_CAPTURE", "portal") };
    let screens = sources();
    assert!(!screens.is_empty());
    for import in [
        None,
        Some(vr::ImportFormats {
            host_alignment: Some(4096),
            ..Default::default()
        }),
    ] {
        let lent = import.is_some();
        let stream = Stream::start_with(screens[0].clone(), 60, import).unwrap();
        let first = frame(&stream, Duration::from_secs(120));
        eprintln!(
            "{}: {}x{} external={:?}",
            screens[0].name, first.width, first.height, first.external
        );
        assert!(first.width > 0 && first.height > 0);
        if !lent {
            assert_eq!(first.rgba.len(), (first.width * first.height * 4) as usize);
        }
        stream
            .touch(Touch {
                id: 0,
                phase: TouchPhase::Move,
                x: 0.5,
                y: 0.5,
            })
            .unwrap();
    }

    // ポインタの位置: RemoteDesktop で動かした位置を、表示サーバーがカーソルのメタデータで知らせる。
    let stream = Stream::start(screens[0].clone()).unwrap();
    let shot = frame(&stream, Duration::from_secs(10));
    let (w, h) = (shot.width as f32, shot.height as f32);
    for (x, y) in [(0.25, 0.5), (0.75, 0.25), (0.5, 0.5)] {
        stream
            .touch(Touch {
                id: 0,
                phase: TouchPhase::Move,
                x,
                y,
            })
            .unwrap();
        // 送った画素と同じ画素に置かれる。
        let want = (((w - 1.0) * x).round(), ((h - 1.0) * y).round());
        let reported = until("the reported pointer", Duration::from_secs(5), || {
            stream
                .pointer()
                .filter(|p| ((p.0 * (w - 1.0)).round(), (p.1 * (h - 1.0)).round()) == want)
        });
        eprintln!("pointer at {x}, {y}: reported {reported:?}");
    }
    // カーソルの画像: 映像に焼き込まれず、画像とホットスポットが届く。
    let cursor = until("the cursor", Duration::from_secs(5), || stream.cursor());
    let (cw, ch) = (cursor.image.width, cursor.image.height);
    eprintln!("cursor: {cw}x{ch}, hotspot {:?}", cursor.hotspot);
    assert!((8..=256).contains(&cw) && (8..=256).contains(&ch));
    assert!(cursor.hotspot.0 < cw && cursor.hotspot.1 < ch);
    let alpha: Vec<u8> = cursor
        .image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .collect();
    assert!(
        alpha.contains(&255) && alpha.contains(&0),
        "an opaque arrow on a clear background"
    );
}

/// 届いたキーの押下を、今の配列で文字（keysym）に戻す。Shift の押下中は 2 段目を読む。
fn typed(conn: &impl Connection, events: &[Event]) -> Vec<u32> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let map = conn
        .get_keyboard_mapping(min, max - min + 1)
        .unwrap()
        .reply()
        .unwrap();
    let per = map.keysyms_per_keycode as usize;
    let syms = |k: u8| &map.keysyms[(k - min) as usize * per..][..per];
    let mut shift = false;
    let mut out = Vec::new();
    for event in events {
        match event {
            Event::KeyPress(e) if [0xffe1, 0xffe2].contains(&syms(e.detail)[0]) => shift = true,
            Event::KeyRelease(e) if [0xffe1, 0xffe2].contains(&syms(e.detail)[0]) => shift = false,
            Event::KeyPress(e) => {
                let s = syms(e.detail);
                out.push(if shift && s[1] != 0 { s[1] } else { s[0] });
            }
            _ => {}
        }
    }
    out
}

/// X11: OS キーボードは XTest でフォーカス中のウィンドウへ、キー（修飾キー付き）と、配列に無い文字を含む文字列を送る。
#[test]
#[ignore = "starts a rootful Xwayland on the desktop"]
fn x11_types_keys_and_text_into_the_focused_window() {
    use os::keyboard::{Output, chord};
    let display = ":58";
    let _server = Server(
        Command::new("Xwayland")
            .args([display, "-geometry", "320x240", "-noreset"])
            .spawn()
            .expect("Xwayland"),
    );
    let (conn, screen) = until("Xwayland", Duration::from_secs(10), || {
        x11rb::connect(Some(display)).ok()
    });
    // SAFETY: このテストは単独で実行する（--test-threads=1）。
    unsafe {
        std::env::set_var("DISPLAY", display);
        std::env::set_var("NYAW_CAPTURE", "x11");
    }
    let root = conn.setup().roots[screen].root;
    let window = conn.generate_id().unwrap();
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        320,
        240,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new()
            .override_redirect(1)
            .event_mask(EventMask::KEY_PRESS | EventMask::KEY_RELEASE),
    )
    .unwrap();
    conn.map_window(window).unwrap();
    conn.get_input_focus().unwrap().reply().unwrap();
    conn.set_input_focus(InputFocus::POINTER_ROOT, window, x11rb::CURRENT_TIME)
        .unwrap();
    conn.get_input_focus().unwrap().reply().unwrap();

    let keyboard = desktop::os_keyboard();
    let settings = until("the focused window", Duration::from_secs(5), || {
        let s = keyboard.settings();
        (s.foreground == u64::from(window)).then_some(s)
    });
    assert!(settings.error.is_none(), "{:?}", settings.error);
    // キーの押下・解放だけを数える（Xwayland は起動直後に配列の変更を通知する）。
    let events = |n: usize| {
        let mut got = Vec::new();
        until("the keys", Duration::from_secs(5), || {
            while let Some(e) = conn.poll_for_event().unwrap() {
                if matches!(e, Event::KeyPress(_) | Event::KeyRelease(_)) {
                    got.push(e);
                }
            }
            (got.len() >= n).then_some(())
        });
        got
    };

    // Shift + A（evdev 42 と 30）
    keyboard
        .send(settings.foreground, Output::Keys(chord(&[42], 30)))
        .unwrap();
    let got = events(4);
    assert_eq!(typed(&conn, &got), [u32::from(b'A')]);

    // 配列にある文字（H は Shift 付き）と、無い文字（é、語）。
    keyboard
        .send(settings.foreground, Output::Text("Hé語".into()))
        .unwrap();
    let got = events(8);
    assert_eq!(typed(&conn, &got), [u32::from(b'H'), 0xe9, 0x0100_8a9e]);
    assert!(keyboard.settings().error.is_none());
}
