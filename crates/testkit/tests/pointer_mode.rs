//! E2E for the pointer mode (mouse / touch emulation). Operates the PointerTest screen with the VR laser.
//! The default is mouse (moving while held is a drag). Switching to touch with `pointer-mode` makes moving while held a swipe,
//! and releasing without moving a tap.

use gpui::TestAppContext;
use kernel::PointerMode;
use testing::{BUTTON_ACTIVE_BG, BUTTON_BG, PointerTest};
use testkit::{Action, Driver, Vec2, VrKit};

/// Presses toward the bottom of `swipe-list`, moves up gradually by 1/2 of its height, and releases.
fn swipe_up(kit: &mut VrKit<PointerTest>) {
    let b = kit.bounds("swipe-list");
    let (x, y0, h) = (
        f32::from(b.center().x),
        f32::from(b.center().y),
        f32::from(b.size.height),
    );
    kit.point_at(Vec2::new(x, y0 + h / 4.0))
        .press(Action::Select);
    for i in 1..=8 {
        kit.point_at(Vec2::new(x, y0 + h / 4.0 - h / 2.0 * i as f32 / 8.0));
    }
    kit.up();
}

fn scrolled(kit: &mut VrKit<PointerTest>) -> f32 {
    kit.read(|v| f32::from(v.list.offset().y))
}

#[gpui::test]
fn mouse_mode_drags_and_does_not_swipe(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| PointerTest::default());
    assert_eq!(
        kit.kernel.pointer_mode(),
        PointerMode::Mouse,
        "mouse by default"
    );
    assert_eq!(kit.color_at("pointer-mode"), BUTTON_BG);
    kit.drag("drag-chip", "drop-zone");
    assert_eq!(kit.read(|v| v.dropped), 1);
    swipe_up(&mut kit);
    assert_eq!(
        scrolled(&mut kit),
        0.0,
        "with the mouse, moving while held does not scroll"
    );
    kit.click("tap-me");
    assert_eq!(kit.read(|v| v.taps), 1);
}

#[gpui::test]
fn touch_mode_swipes_and_taps(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| PointerTest::default());
    kit.click("pointer-mode");
    assert_eq!(kit.kernel.pointer_mode(), PointerMode::Touch);
    // With the laser on it the color becomes the hover color, so move it off before looking.
    assert_eq!(
        kit.hover("tap-me").color_at("pointer-mode"),
        BUTTON_ACTIVE_BG
    );

    swipe_up(&mut kit);
    let s = scrolled(&mut kit);
    assert!(s < -50.0, "scrolls down following the finger ({s})");
    kit.click("tap-me");
    assert_eq!(kit.read(|v| v.taps), 1, "releasing without moving is a tap");
    kit.drag("drag-chip", "drop-zone");
    assert_eq!(kit.read(|v| v.dropped), 0, "touch does not make a drag");

    // Even in touch mode it can be switched back with the button.
    kit.click("pointer-mode");
    assert_eq!(kit.kernel.pointer_mode(), PointerMode::Mouse);
    assert_eq!(kit.hover("tap-me").color_at("pointer-mode"), BUTTON_BG);
}
