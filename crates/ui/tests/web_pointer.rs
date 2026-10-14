//! E2E for pointer modes (mouse / touch emulation) run in the real platform's WebView (mock VR).
//! Opens the React Pointer test and checks that with the default mouse, moving while pressed does not scroll,
//! that switching to touch via the page button makes pointerType touch, swiping scrolls (pointercancel arrives),
//! releasing without moving is a tap (click), and dragging continues over touch-action: none, via JS-side events.
//! On Linux, with a display (Xvfb is fine), `cargo test -p ui --test web_pointer` runs it.

mod common;

use std::process::ExitCode;
use std::time::Duration;

use common::{PANEL_POS, Page, SharedMock, Vr, click, num, press};
use kernel::vr::Quat;
use kernel::{Kernel, Pose, Vec2, VrBackend};
use ui::PanelSpec;

fn main() -> ExitCode {
    if !common::should_run("web_pointer") {
        return ExitCode::SUCCESS;
    }
    let mock = SharedMock::default();
    // turn off hand-shake damping, since it makes pointing miss right after aiming (same as testkit).
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        // write the scenarios for mouse (the default touch is covered by web_pointer).
        pointer_mode: Some(kernel::PointerMode::Mouse),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(Box::new(mock.clone()) as Box<dyn VrBackend>, config);
    let spec = PanelSpec::web(Pose::new(PANEL_POS, Quat::IDENTITY), webpages::TESTING)
        .sized(Vec2::new(0.8, 0.6), (800, 600));
    common::run(
        "web_pointer",
        kernel,
        vec![spec],
        "panel-0",
        None,
        move |page, _| {
            scenario(page, &mut Vr::new(mock));
        },
    )
}

fn scenario(page: &mut Page, vr: &mut Vr) {
    press(page, vr, "open-pointer");
    page.until("Pointer test opens", |s| {
        s["state"]["page"] == "pointer" && !s["state"]["status"].is_null()
    });
    assert_eq!(
        page.probe()["state"]["status"]["pointer_mode"],
        "mouse",
        "this test starts with the mouse"
    );

    // mouse: moving while pressed does not scroll (just press, move, release).
    swipe_up(page, vr);
    let s = page.until("released", |s| {
        num(s, &["state", "counts", "pointerup"]) >= 1.0
    });
    assert_eq!(s["state"]["lastPointer"]["pointerType"], "mouse");
    assert_eq!(
        num(&s, &["state", "scrollTop"]),
        0.0,
        "mouse does not scroll"
    );
    assert_eq!(num(&s, &["state", "counts", "pointercancel"]), 0.0);
    stick_scroll(page, vr);

    // switch to touch (page -> Tauri command -> kernel -> status event).
    click(page, vr, "pointer-mode");
    page.until("switched to touch", |s| {
        s["state"]["status"]["pointer_mode"] == "touch"
    });

    // swipe: content follows the finger and scrolls down, and the pointer is cancelled.
    swipe_up(page, vr);
    let s = page.until("scrolled by swipe", |s| {
        num(s, &["state", "scrollTop"]) > 50.0
    });
    assert_eq!(num(&s, &["state", "counts", "pointercancel"]), 1.0);
    assert_eq!(s["state"]["lastPointer"]["pointerType"], "touch");

    // a 2D area also moves both ways with a diagonal swipe.
    let (x, y) = page.center("swipe-grid");
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.point_at((x + 60.0, y + 80.0));
    vr.set("trigger/value", 1.0);
    page.until("touched the 2D area", |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
    });
    glide(vr, (x + 60.0, y + 80.0), (x - 60.0, y - 80.0));
    vr.set("trigger/value", 0.0);
    page.until("2D swipe", |s| {
        let (left, top) = scroll_position(s, "swipe-grid");
        left > 50.0 && top > 50.0
    });
    stick_scroll(page, vr);

    // tap: releasing without moving gives a click.
    tap(page, vr, "tap-me");
    let s = page.until("tapped", |s| s["state"]["taps"] == 1);
    assert_eq!(s["state"]["lastPointer"]["pointerType"], "touch");

    // over touch-action: none it does not scroll and can be dragged.
    let (chip, zone) = (page.center("drag-chip"), page.center("drop-zone"));
    vr.point_at(chip);
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 1.0);
    page.until("pressed the chip", |s| {
        s["state"]["lastPointer"]["target"] == "drag-chip"
    });
    glide(vr, chip, zone);
    vr.set("trigger/value", 0.0);
    page.until("can drag with touch too", |s| s["state"]["dropped"] == 1);

    // can switch back to mouse with the button while in touch.
    tap(page, vr, "pointer-mode");
    page.until("back to mouse", |s| {
        s["state"]["status"]["pointer_mode"] == "mouse"
    });
}

fn scroll_position(s: &serde_json::Value, id: &str) -> (f64, f64) {
    if id == "swipe-list" {
        (
            num(s, &["state", "scrollLeft"]),
            num(s, &["state", "scrollTop"]),
        )
    } else {
        (
            s["state"]["scroll2d"][0].as_f64().unwrap(),
            s["state"]["scroll2d"][1].as_f64().unwrap(),
        )
    }
}

/// the stick moves both ways in either mode, and tilting back returns to the edge.
fn stick_scroll(page: &mut Page, vr: &mut Vr) {
    for id in ["swipe-list", "swipe-grid"] {
        vr.point_at(page.center(id));
        let before = scroll_position(&page.probe(), id);
        vr.set("thumbstick/x", 1.0);
        vr.set("thumbstick/y", -1.0);
        let s = page.until("scroll with stick", |s| {
            let (left, top) = scroll_position(s, id);
            top > before.1 && (id == "swipe-list" || left > before.0)
        });
        if id == "swipe-list" {
            assert_eq!(scroll_position(&s, id).0, 0.0);
        }
        vr.set("thumbstick/x", -1.0);
        vr.set("thumbstick/y", 1.0);
        page.until("back to the edge in reverse", |s| {
            scroll_position(s, id) == (0.0, 0.0)
        });
        vr.set("thumbstick/x", 0.0);
        vr.set("thumbstick/y", 0.0);
    }
}

/// press and release the element's center by touch. Touch has no hover events before contact, so check the press via pointerdown.
fn tap(page: &mut Page, vr: &mut Vr, id: &str) {
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.point_at(page.center(id));
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 1.0);
    page.until(&format!("touched {id}"), |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
            && s["state"]["lastPointer"]["target"] == id
    });
    vr.set("trigger/value", 0.0);
}

/// press the lower part of `swipe-list`, move up 200px in small steps, and release.
fn swipe_up(page: &mut Page, vr: &mut Vr) {
    let (x, y) = page.center("swipe-list");
    let ups = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.point_at((x, y + 100.0));
    std::thread::sleep(Duration::from_millis(50));
    vr.set("trigger/value", 1.0);
    page.until("pressed the list", |s| {
        num(s, &["state", "counts", "pointerdown"]) > ups
    });
    glide(vr, (x, y + 100.0), (x, y - 100.0));
    vr.set("trigger/value", 0.0);
}

/// move the laser from `from` to `to` in small steps.
fn glide(vr: &mut Vr, from: (f32, f32), to: (f32, f32)) {
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        vr.point_at((from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t));
        std::thread::sleep(Duration::from_millis(15));
    }
}
