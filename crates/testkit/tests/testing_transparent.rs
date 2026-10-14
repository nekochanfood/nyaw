//! testing-transparent screen: the background reaches the VR panel semi-transparent, and the Close button at top right closes only itself.

use gpui::TestAppContext;
use testing::TestingTransparent;
use testkit::{Driver, VrKit};

#[gpui::test]
fn background_is_translucent_in_vr(cx: &mut TestAppContext) {
    let kit = VrKit::new(cx, |_, _| TestingTransparent::new(0));
    let [_, _, _, a] = kit.texture().pixel(2, 2);
    assert!(0 < a && a < 255, "should be semi-transparent: alpha={a}");
}

#[gpui::test]
fn close_button_is_top_right_and_closes_panel(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| TestingTransparent::new(0));
    let b = kit.bounds("close-transparent");
    let (w, _) = kit.kernel.panel(0).resolution;
    assert!(f32::from(b.origin.x) > w as f32 / 2.0, "toward the right");
    assert!(f32::from(b.origin.y) < 40.0, "toward the top");

    assert!(!kit.kernel.panel(0).closed);
    kit.click("close-transparent");
    assert!(kit.kernel.panel(0).closed);
}
