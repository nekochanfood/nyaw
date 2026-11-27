//! Smoke test of the tutorial on the production frame loop (`ui::run_with`) with the real wrist, dashboard, beam and setup
//! wizard on the novr emulator, rendered on the real GPU. It goes through setup, opens the dashboard with X twice (which
//! starts the tutorial), passes every step with Next, and finishes. Each step's emulator screen goes to
//! `target/tmp/tutorial-*.bmp` to look at. Runs only with `--ignored` on a machine with a display.

use crate::common;

use std::process::ExitCode;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Driven, Emu};
use dashboard::CoachSpot;
use kernel::vr::novr::Button;
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, Vec2, VrBackend};

const SCREEN: (u32, u32) = (1280, 720);
/// Where setup puts its wizard (1.5m ahead of the eye at the origin) and its size in px (1px = 1mm).
const WIZARD: Vec3 = Vec3::new(0.0, 0.0, -1.5);
const WIZARD_PX: (f32, f32) = (1152.0, 864.0);
/// The pages of setup before Done in novr (no SteamVR step).
const SETUP_PAGES: usize = 5;
/// Steps of the tour from the first screen up to Done, and where the coach stands at each when only Next is pressed:
/// docked at first, above the bar while nothing is docked, beside the docked screen (lower by the screen's bar).
const STEPS: [(&str, CoachSpot); 14] = [
    ("intro", CoachSpot::Docked),
    ("bar", CoachSpot::Bar),
    ("toggle", CoachSpot::Bar),
    ("menu", CoachSpot::Bar),
    ("quick-settings", CoachSpot::Bar),
    ("grab", CoachSpot::Beside),
    ("dock", CoachSpot::Beside),
    ("nav-bar", CoachSpot::Below),
    // Quick settings is still docked (Next didn't close it).
    ("launcher", CoachSpot::Beside),
    ("pick", CoachSpot::Beside),
    ("operate", CoachSpot::Beside),
    ("minimize", CoachSpot::Below),
    // The practice window is put away, so the slot is free.
    ("recall", CoachSpot::Bar),
    ("done", CoachSpot::Bar),
];

pub fn main() -> ExitCode {
    if !common::should_run("tutorial") {
        return ExitCode::SUCCESS;
    }
    let (backend, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    // As in `main`: the wrist (with the dashboard and the toast), the beam, and setup on a first run.
    let mut panels = vec![wrist::spec(
        &kernel.settings(),
        |from, opened| testing::spec(from, opened),
        |from, opened| settings::spec(from, opened),
    )];
    panels.extend(beam::specs());
    panels.push(setup::spec());
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&emu, &remote)))
            .is_ok();
        println!("tutorial: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, hooks);
    common::exit_code("tutorial", &result)
}

/// The screen point of pixel `(x, y)` on a panel at `pose` that is `size` px and `width` m wide.
fn on_panel(emu: &Emu, pose: Pose, size: (f32, f32), width: f32, (x, y): (f32, f32)) -> Vec2 {
    let k = width / size.0;
    let local = Vec3::new((x - size.0 / 2.0) * k, (size.1 / 2.0 - y) * k, 0.0);
    let world = pose.transform_point(local);
    emu.run(move |e| e.project(world))
        .expect("the panel is in front of the eye")
}

fn click(emu: &Emu, at: Vec2) {
    emu.point(Some(at));
    std::thread::sleep(Duration::from_millis(150));
    emu.button(Button::Trigger, true);
    std::thread::sleep(Duration::from_millis(80));
    emu.button(Button::Trigger, false);
    // Let the page change and the panels settle.
    std::thread::sleep(Duration::from_millis(900));
}

fn shoot(emu: &Emu, name: &str) -> (u32, u32, Vec<u8>) {
    let (width, height, rgba) = emu.run(|e| e.screenshot());
    let frame = ui::Frame {
        width,
        height,
        rgba: rgba.clone(),
    };
    crate::look::save(&frame, &format!("target/tmp/tutorial-{name}.bmp"));
    (width, height, rgba)
}

fn color((w, _, rgba): &(u32, u32, Vec<u8>), at: Vec2) -> [u8; 3] {
    let i = ((at.y as u32 * w + at.x as u32) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

fn scenario(emu: &Emu, remote: &std::sync::mpsc::Sender<ui::Remote>) {
    // Startup and the first renders.
    std::thread::sleep(Duration::from_secs(4));
    let wizard = Pose::new(WIZARD, Quat::IDENTITY);
    // "Next" sits at the lower right of every page (margin 24 + padding 24, at least 135 px wide).
    let next = on_panel(
        emu,
        wizard,
        WIZARD_PX,
        WIZARD_PX.0 / 1000.0,
        (1040.0, WIZARD_PX.1 - 48.0 - 27.0),
    );
    for _ in 0..SETUP_PAGES {
        click(emu, next);
    }
    shoot(emu, "00-setup-done");
    emu.point(None);
    // X twice opens the dashboard and finishes setup; the dashboard starts the tutorial.
    for down in [true, false, true, false] {
        emu.button(Button::X, down);
        std::thread::sleep(Duration::from_millis(40));
    }
    std::thread::sleep(Duration::from_secs(2));

    // The eye is the origin and the dashboard's settings are the defaults. "Next" (and "Start", "Done") sits at the lower
    // right of the coach: margin 24 + padding 24, at least 135 px wide, 54 px high.
    let size = (dashboard::COACH_PX.0 as f32, dashboard::COACH_PX.1 as f32);
    let coach = |spot| dashboard::coach_pose(spot, kernel::DASHBOARD_DISTANCE, 0.0);
    let forward = |spot| {
        on_panel(
            emu,
            coach(spot),
            size,
            dashboard::COACH_M.x,
            (size.0 - 48.0 - 50.0, size.1 - 48.0 - 27.0),
        )
    };
    let running = || {
        let (tx, rx) = channel();
        let _ = remote.send(Box::new(move |cx| {
            let _ = tx.send(ui::tour::running(cx));
        }));
        rx.recv_timeout(common::TIMEOUT).expect("no answer")
    };
    assert!(running(), "the tutorial did not start");
    let card = on_panel(
        emu,
        coach(CoachSpot::Bar),
        size,
        dashboard::COACH_M.x,
        (size.0 / 2.0, 60.0),
    );
    let mut during = None;
    for (i, (step, spot)) in STEPS.iter().enumerate() {
        let shot = shoot(emu, &format!("{:02}-{step}", i + 1));
        if *step == "done" {
            during = Some(shot);
        }
        click(emu, forward(*spot));
    }
    // Done closes the coach and the practice window and ends the tour.
    let after = shoot(emu, "15-finished");
    assert!(!running(), "the tutorial did not end");
    let during = during.expect("reached Done");
    assert_ne!(
        color(&during, card),
        color(&after, card),
        "the coach is still shown"
    );
}
