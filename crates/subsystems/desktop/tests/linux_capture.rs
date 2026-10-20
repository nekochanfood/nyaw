//! Portal の共有ダイアログを操作できる実デスクトップで明示的に実行する。
#![cfg(target_os = "linux")]

use desktop::{SourceKind, Sources, Status, Stream};
use std::{
    thread,
    time::{Duration, Instant},
};

fn wait<T>(mut poll: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(value) = poll() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "portal selection or frame timed out"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn capture(kind: SourceKind) {
    let sources = Sources::refresh().unwrap();
    let sources = wait(|| sources.poll()).unwrap();
    let source = sources
        .into_iter()
        .find(|s| s.kind == kind)
        .expect("portal supports requested capture kind");
    let stream = Stream::start(source).unwrap();
    let frame = wait(|| {
        let update = stream.poll()?;
        assert!(
            !matches!(update.status, Status::Failed(_) | Status::Closed),
            "{:?}",
            update.status
        );
        update.frame
    });
    assert!(frame.width > 0 && frame.height > 0);
    assert_eq!(
        frame.rgba.len(),
        frame.width as usize * frame.height as usize * 4
    );
    assert!(frame.rgba.iter().skip(3).step_by(4).any(|alpha| *alpha > 0));
    // Drop で停止を伝え、ワーカーに Portal セッションと PipeWire 資源を解放させる。
    drop(stream);
}

#[test]
#[ignore = "requires an interactive Linux desktop; select a screen in the portal dialog (remote interaction is optional)"]
fn portal_screen_produces_frames() {
    capture(SourceKind::Screen);
}

#[test]
#[ignore = "requires an interactive Linux desktop; select a window in the portal dialog (remote interaction is optional)"]
fn portal_window_produces_frames() {
    capture(SourceKind::Window);
}
