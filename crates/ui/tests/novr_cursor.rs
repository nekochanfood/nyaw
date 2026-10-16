//! E2E for the pointer's look in novr (emulator). Runs GPUI's Testing and the beam on the production `ui::run_with`,
//! checking that holding a touch without moving fills a long-press progress ring (donut-shaped; the circle fits in the hole) around the cursor,
//! and that it shows on the emulator screen. It renders on the real GPU and reads back, so it only runs with `--ignored` on a machine with a display.

mod common;

use std::f32::consts::TAU;
use std::process::ExitCode;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Driven, Emu};
use kernel::vr::novr::Button;
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, Vec2, VrBackend};
use testing::Testing;
use ui::PanelSpec;

const SCREEN: (u32, u32) = (1280, 720);
const PANEL: Vec3 = Vec3::new(0.0, 0.0, -1.0);

fn main() -> ExitCode {
    if !common::should_run("novr_cursor") {
        return ExitCode::SUCCESS;
    }
    let (backend, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    // keep the default (touch).
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    let mut panels = vec![PanelSpec::new(
        Pose::new(PANEL, Quat::IDENTITY),
        |id, _, _| Testing::new(id),
    )];
    panels.extend(beam::specs());
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&emu))).is_ok();
        println!("novr_cursor: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, hooks);
    common::exit_code("novr_cursor", &result)
}

fn brightness(c: [u8; 3]) -> u32 {
    c.iter().map(|&v| v as u32).sum::<u32>() / 3
}

fn scenario(emu: &Emu) {
    // wait for startup and the panel's first render.
    std::thread::sleep(Duration::from_secs(2));
    // point at the lower right of the panel (background with no buttons).
    let at = emu
        .run(|e| e.project(PANEL + Vec3::new(0.3, -0.2, 0.0)))
        .expect("panel is in front of the eyes");
    emu.point(Some(at));
    std::thread::sleep(Duration::from_millis(300));
    // ring radius (screen px): the cursor panel is 36mm at 1m, and the ring's center line is 1/3 of that (80/240 px).
    let ring = emu.run(move |e| {
        let edge = e
            .project(PANEL + Vec3::new(0.3 + 0.036 / 3.0, -0.2, 0.0))
            .unwrap();
        edge.x - at.x
    });
    let on_ring = |turn: f32| {
        let a = (turn - 0.25) * TAU;
        at + Vec2::new(ring * a.cos(), ring * a.sin())
    };
    // no ring before pressing (the 12 o'clock position is still panel background).
    let before = emu.color(on_ring(0.02));

    emu.button(Button::Trigger, true);
    // the ring stays hidden for a moment after pressing, and shows a few frames later (later with a software GPU),
    // so wait until the filled side (just past 12 o'clock) turns bright instead of a fixed time.
    let start = std::time::Instant::now();
    let mut filled = emu.color(on_ring(0.08));
    while brightness(filled) <= brightness(before) + 40 && start.elapsed() < Duration::from_secs(1)
    {
        std::thread::sleep(Duration::from_millis(5));
        filled = emu.color(on_ring(0.08));
    }
    // partly filled: the unfilled side (around 10 o'clock) is still the dark base.
    let empty = emu.color(on_ring(0.85));
    emu.button(Button::Trigger, false);
    println!(
        "ring: before press {before:02x?} / filled side {filled:02x?} / unfilled side {empty:02x?}"
    );
    assert!(
        brightness(filled) > brightness(before) + 40,
        "filled arc shows bright ({before:02x?} -> {filled:02x?})"
    );
    assert!(
        brightness(filled) > brightness(empty) + 40,
        "filled and unfilled sides differ ({filled:02x?} / {empty:02x?})"
    );
    // the ring disappears on release.
    std::thread::sleep(Duration::from_millis(200));
    let after = emu.color(on_ring(0.08));
    assert!(
        brightness(after) + 40 < brightness(filled),
        "disappears on release ({after:02x?})"
    );
}
