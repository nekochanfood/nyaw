//! Home パネルのユーザー操作シナリオ。

use gpui::TestAppContext;
use testkit::{Hand, Kit, Vec3};
use ui::Home;

#[gpui::test]
fn click_button(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    kit.click("click-me").click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 2);
}

#[gpui::test]
fn hover_alone_does_not_click(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    kit.hover("click-me");
    assert_eq!(kit.read(|h| h.clicks), 0);
}

#[gpui::test]
fn every_controller_can_click(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    let profiles = [
        "khr/simple_controller",
        "valve/index_controller",
        "oculus/touch_controller",
        "htc/vive_controller",
        "acme/unknown_controller",
    ];
    for p in profiles {
        kit.profile(p).click("click-me");
    }
    assert_eq!(kit.read(|h| h.clicks), profiles.len());
}

#[gpui::test]
fn left_hand_clicks(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    kit.hand(Hand::Left)
        .profile("oculus/touch_controller")
        .click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn dragged_panel_moves_in_vr_and_stays_clickable(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    let to = Vec3::new(0.3, 0.2, -1.2);
    kit.drag_panel(to);
    assert!(kit.panel_pose().position.abs_diff_eq(to, 1e-4));
    assert!(kit.vr.overlays[0].pose.position.abs_diff_eq(to, 1e-4));

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
