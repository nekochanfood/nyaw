//! E2E for the test panels on the production `ui::run_with` (novr emulator, no window).
//! - testing-anim keeps changing on screen without any input (the frame loop redraws `live` panels every frame).
//! - Summoning more panels than allowed stops exactly at the limit (`PanelControl::limit`), and `open` reports it.
//!
//! It renders on the real GPU and reads back, so it only runs with `--ignored` on a machine with a display.

mod common;

use std::process::ExitCode;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Driven, Emu};
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, VrBackend};
use testing::{TestingAnim, TestingTransparent};
use ui::{MAX_PANELS, PanelControl, PanelSpec};

const SCREEN: (u32, u32) = (640, 360);
const PANEL: Vec3 = Vec3::new(0.0, 0.0, -1.0);

fn main() -> ExitCode {
    if !common::should_run("test_panels") {
        return ExitCode::SUCCESS;
    }
    let (backend, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    let panels = vec![PanelSpec::new(
        Pose::new(PANEL, Quat::IDENTITY),
        |id, _, cx| TestingAnim::new(id, cx),
    )];
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&emu, &remote)))
            .is_ok();
        println!("test_panels: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        on_web: None,
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, webpages::FILES, hooks);
    common::exit_code("test_panels", &result)
}

/// Runs `f` on the app and waits for its result.
fn on_app<R: Send + 'static>(
    remote: &Sender<ui::Remote>,
    f: impl FnOnce(&mut gpui::App) -> R + Send + 'static,
) -> R {
    let (tx, rx) = channel();
    remote
        .send(Box::new(move |cx| tx.send(f(cx)).unwrap()))
        .unwrap();
    rx.recv_timeout(Duration::from_secs(10)).unwrap()
}

fn open_and_limit(remote: &Sender<ui::Remote>) -> (usize, usize) {
    on_app(remote, |cx| {
        let c = cx.default_global::<PanelControl>();
        (c.open, c.limit)
    })
}

fn scenario(emu: &Emu, remote: &Sender<ui::Remote>) {
    std::thread::sleep(Duration::from_secs(2));
    // A row across the panel through the moving square (y 400-560 of 600 px; the panel is 0.6m tall).
    let at = emu
        .run(|e| e.project(PANEL + Vec3::new(0.0, -0.18, 0.0)))
        .expect("panel is in front of the eyes");
    let pixels = move |e: &mut kernel::vr::novr::Emulator| {
        let (w, _, rgba) = e.screenshot();
        let row = at.y as usize * w as usize * 4;
        rgba[row..row + w as usize * 4].to_vec()
    };
    let before = emu.run(pixels);
    std::thread::sleep(Duration::from_millis(300));
    assert!(emu.run(pixels) != before, "testing-anim moves without input");

    let (open, limit) = open_and_limit(remote);
    assert_eq!((open, limit), (1, MAX_PANELS), "novr has no runtime limit");
    on_app(remote, move |cx| {
        let c = cx.default_global::<PanelControl>();
        for i in 0..limit - open + 5 {
            c.spawns.push(
                PanelSpec::new(Pose::IDENTITY, |id, _, _| TestingTransparent::new(id)).in_view(
                    Pose::new(Vec3::new(i as f32 * 0.01, 0.0, -2.0), Quat::IDENTITY),
                ),
            );
        }
    });
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        open_and_limit(remote),
        (MAX_PANELS, MAX_PANELS),
        "stops at the limit"
    );
}
