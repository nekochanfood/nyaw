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
        xproto::{ConnectionExt as _, CreateWindowAux, EventMask, WindowClass},
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
}
