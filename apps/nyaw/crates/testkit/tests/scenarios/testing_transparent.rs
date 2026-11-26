//! testing-transparent screen: the background reaches the VR panel semi-transparent.

use gpui::TestAppContext;
use testing::TestingTransparent;
use testkit::VrKit;

#[gpui::test]
fn background_is_translucent_in_vr(cx: &mut TestAppContext) {
    let kit = VrKit::new(cx, |_, _| TestingTransparent::new(0));
    let [_, _, _, a] = kit.texture().pixel(2, 2);
    assert!(0 < a && a < 255, "should be semi-transparent: alpha={a}");
}
