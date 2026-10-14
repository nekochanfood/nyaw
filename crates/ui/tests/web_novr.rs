//! E2E for novr (emulator). Opens GPUI's Testing on the production `ui::run_with`, then opens the web version of Testing from it
//! (like the button, pushes `PanelSpec::spawn_web` onto `PanelControl::spawns`; the button itself is testkit's `open_web_testing`).
//! As in production novr, the WebView is rendered off-screen and read back, then composited onto the emulator screen. Input comes from the emulator's
//! mouse (right-hand controller): left click = trigger, right click = grip, arrow keys = stick. It reaches the page as the bridge's synthetic events.
//! The emulator shows no window (headless) and is driven from the test thread via `common::Emu`.
//! On Linux, with a display (Xvfb is fine), `cargo test -p ui --test web_novr` runs it.

mod common;

use std::process::ExitCode;
use std::time::Instant;

use common::{Driven, Emu, Page, num};
use kernel::vr::novr::Button;
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, Vec2, VrBackend};
use serde_json::Value;
use ui::{PanelControl, PanelSpec};

/// position of GPUI's Testing (panel 0). The web version (panel 1) opens stacked 5cm in front of its center.
const GPUI_POS: Vec3 = Vec3::new(-0.6, 0.0, -1.5);
const WEB_POS: Vec3 = Vec3::new(-0.6, 0.0, -1.45);

/// the screen point where a point on the web panel (CSS px) appears. Size and resolution are read from the status the page received.
fn on_screen(emu: &Emu, status: &Value, (x, y): (f32, f32)) -> Vec2 {
    let f = |v: &Value| v.as_f64().unwrap() as f32;
    let (sw, sh) = (f(&status["size"][0]), f(&status["size"][1]));
    let (rw, rh) = (f(&status["resolution"][0]), f(&status["resolution"][1]));
    let p = WEB_POS + Vec3::new((x / rw - 0.5) * sw, (0.5 - y / rh) * sh, 0.0);
    emu.run(move |e| e.project(p))
        .expect("panel is in front of the eyes")
}

fn near(rgb: [u8; 3]) -> impl Fn([u8; 3]) -> bool {
    move |c| {
        c.iter()
            .zip(rgb)
            .all(|(a, b)| (*a as i32 - b as i32).abs() <= 8)
    }
}

fn gray(c: [u8; 3]) -> bool {
    c[0] == c[1] && c[1] == c[2]
}

fn main() -> ExitCode {
    if !common::should_run("web_novr") {
        return ExitCode::SUCCESS;
    }
    let (emu, tx) = Driven::headless(1280, 720);
    // turn off hand-shake damping, since it makes pointing miss right after aiming (same as testkit).
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        // write the scenarios for mouse (the default touch is covered by web_pointer).
        pointer_mode: Some(kernel::PointerMode::Mouse),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(Box::new(emu) as Box<dyn VrBackend>, config);
    // open the web version (panel 1) from GPUI's Testing (panel 0).
    let gpui = PanelSpec::new(Pose::new(GPUI_POS, Quat::IDENTITY), |id, _, _| {
        testing::Testing::new(id)
    });
    let open_web: ui::Remote = Box::new(|cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn_web(0, testing::WEB_PAGE))
    });
    common::run(
        "web_novr",
        kernel,
        vec![gpui],
        "panel-1",
        Some(open_web),
        move |page, _| scenario(page, &tx),
    )
}

/// point at the element's center with the mouse.
fn hover(page: &mut Page, emu: &Emu, id: &str) -> Vec2 {
    let status = page.probe()["state"]["status"].clone();
    let at = on_screen(emu, &status, page.center(id));
    emu.point(Some(at));
    page.until(&format!("hover over {id}"), |s| {
        s["state"]["last"]["target"] == id
    });
    at
}

/// left-click (trigger) the element's center.
fn click(page: &mut Page, emu: &Emu, id: &str) {
    hover(page, emu, id);
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    emu.button(Button::Trigger, true);
    page.until(&format!("pressed {id}"), |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
    });
    emu.button(Button::Trigger, false);
}

fn scenario(page: &mut Page, emu: &Emu) {
    let s = page.probe();
    assert_eq!(s["state"]["status"]["panel"], 1);
    assert_eq!(s["state"]["status"]["runtime"], "novr");
    let status = s["state"]["status"].clone();
    let h = status["resolution"][1].as_f64().expect("resolution") as f32;

    // the off-screen WebView is read back and shows on the emulator screen (background is opaque #202020).
    // outside the panel (right) is an achromatic checkerboard.
    let w = status["resolution"][0].as_f64().expect("resolution") as f32;
    let bottom_left = on_screen(emu, &status, (4.0, h - 10.0));
    emu.until_color(
        "the page's render shows on screen",
        bottom_left,
        near([0x20, 0x20, 0x20]),
    );
    let outside = on_screen(emu, &status, (w - 4.0, h - 10.0)) + Vec2::new(100.0, 0.0);
    assert!(gray(emu.color(outside)), "outside the panel is achromatic");
    assert_ne!(
        emu.color(Vec2::new(0.0, 0.0)),
        emu.color(Vec2::new(16.0, 0.0)),
        "checkerboard"
    );

    // hover: the mouse direction becomes the right-hand ray and arrives as the bridge's synthetic events (even Tailwind's hover: color shows).
    hover(page, emu, "click-me");
    let s = page.until("mouse hovers", |s| s["state"]["hovered"] == true);
    assert_eq!(
        s["state"]["lastPointer"]["trusted"], false,
        "event synthesized by the bridge"
    );
    let edge = on_screen(emu, &status, page.left_edge("click-me", 3.0));
    emu.until_color(
        "hover color shows on screen",
        edge,
        near([0x50, 0x54, 0x5f]),
    );

    // left click = trigger.
    emu.button(Button::Trigger, true);
    page.until("pressed", |s| {
        num(s, &["state", "counts", "pointerdown"]) == 1.0
    });
    let clicked = Instant::now();
    emu.button(Button::Trigger, false);
    page.until("clicked", |s| s["state"]["clicks"] == 1);
    emu.until_color(
        "click result shows on screen",
        edge,
        near([0x2e, 0xcc, 0x71]),
    );
    // rough response time (from release until it shows on screen, in 20ms polling steps).
    println!(
        "web_novr: click shown on screen after {} ms",
        clicked.elapsed().as_millis()
    );

    // arrow keys = right-hand stick. Tilting down moves the list down.
    // there is a row at the list's center, so don't wait for the target to be the list itself.
    let list = on_screen(emu, &status, page.center("scroll-list"));
    emu.point(Some(list));
    emu.stick(Vec2::new(0.0, -1.0));
    page.until("scrolled", |s| {
        num(s, &["state", "wheel"]) > 0.0 && num(s, &["state", "scrollTop"]) > 0.0
    });
    emu.stick(Vec2::ZERO);

    // interaction with the kernel is the same on every runtime.
    click(page, emu, "lock");
    page.until("locked", |s| s["state"]["status"]["locked"] == true);
    click(page, emu, "lock");
    page.until("unlocked", |s| s["state"]["status"]["locked"] == false);

    // right click = grip: grab the panel and carry it right with the mouse, and it shows where it was outside the panel
    // (the GPUI Testing stacked below is visible at the original spot, so don't check there).
    let grip = on_screen(emu, &status, (20.0, h - 20.0));
    emu.point(Some(grip));
    emu.button(Button::Grab, true);
    page.until("grabbing", |s| s["state"]["status"]["grabbed"] == true);
    for i in 1..=10 {
        emu.point(Some(grip + Vec2::new(20.0 * i as f32, 0.0)));
    }
    emu.until_color("panel moved", outside, near([0x20, 0x20, 0x20]));
    emu.button(Button::Grab, false);
    page.until("released", |s| s["state"]["status"]["grabbed"] == false);

    // when the mouse leaves the screen the right hand disappears and leave arrives.
    emu.point(None);
    page.until("left", |s| s["state"]["hovered"] == false);
}
