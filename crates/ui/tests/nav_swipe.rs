//! E2E for the page stack's swipe back on the production `ui::run_with` (novr emulator, real hidden windows, touch by default).
//! Pushes a page, then holds the trigger and moves right to left, both right away (finger slide) and after a long press
//! (which turns into a mouse press), and checks that the page went back. Only runs with `--ignored` on non-Linux.

mod common;

use std::process::ExitCode;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Driven, Emu};
use kernel::vr::novr::Button;
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, Vec2, VrBackend};
use testing::{NavPage, NavTest};
use ui::PanelSpec;

const SCREEN: (u32, u32) = (1280, 720);
const PANEL: Vec3 = Vec3::new(0.0, 0.0, -1.0);

fn main() -> ExitCode {
    if !common::should_run("nav_swipe") {
        return ExitCode::SUCCESS;
    }
    let (backend, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    let mut panels = vec![PanelSpec::new(Pose::new(PANEL, Quat::IDENTITY), |id, _, _| {
        NavTest::new(id)
    })];
    panels.extend(beam::specs());
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&emu, &remote)))
            .is_ok();
        println!("nav_swipe: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, hooks);
    common::exit_code("nav_swipe", &result)
}

/// Runs `f` on the NavTest view in the frame loop and returns its result.
fn with_view<R: Send + 'static>(
    remote: &Sender<ui::Remote>,
    f: impl FnOnce(&mut NavTest) -> R + Send + 'static,
) -> R {
    let (tx, rx) = channel();
    remote
        .send(Box::new(move |cx| {
            let view = cx
                .windows()
                .into_iter()
                .filter_map(|w| w.downcast::<ui::PanelRoot>())
                .find_map(|w| {
                    w.read(cx)
                        .ok()
                        .and_then(|r| r.view().downcast::<NavTest>().ok())
                })
                .expect("NavTest window");
            let r = view.update(cx, |v, cx| {
                let r = f(v);
                cx.notify();
                r
            });
            let _ = tx.send(r);
        }))
        .expect("frame loop");
    rx.recv_timeout(common::TIMEOUT).expect("frame loop answers")
}

/// Screen point of panel pixel (x, y) (800x600 at 1000px/m).
fn screen(emu: &Emu, x: f32, y: f32) -> Vec2 {
    let world = PANEL + Vec3::new((x - 400.0) / 1000.0, (300.0 - y) / 1000.0, 0.0);
    emu.run(move |e| e.project(world)).expect("in view")
}

/// Holds the trigger at panel pixel (700, 400), optionally waits, moves left by 400px over ~20 frames, releases.
fn swipe(emu: &Emu, hold: Duration) {
    emu.point(Some(screen(emu, 700.0, 400.0)));
    std::thread::sleep(Duration::from_millis(100));
    emu.button(Button::Trigger, true);
    std::thread::sleep(hold);
    for i in 1..=20 {
        emu.point(Some(screen(emu, 700.0 - 20.0 * i as f32, 400.0)));
        std::thread::sleep(Duration::from_millis(12));
    }
    emu.button(Button::Trigger, false);
    std::thread::sleep(Duration::from_millis(500));
}

fn scenario(emu: &Emu, remote: &Sender<ui::Remote>) {
    std::thread::sleep(Duration::from_secs(2));
    for (name, hold) in [
        ("slide", Duration::from_millis(20)),
        ("long press", Duration::from_millis(600)),
    ] {
        with_view(remote, |v| v.stack.push(NavPage::Detail(0)));
        std::thread::sleep(Duration::from_millis(600));
        swipe(emu, hold);
        let (stack, back) = with_view(remote, |v| (v.stack.clone(), v.swiped_back));
        println!("{name}: stack {stack:?} swiped_back {back}");
        assert_eq!(stack, [NavPage::Root], "{name}: went back");
    }
}
