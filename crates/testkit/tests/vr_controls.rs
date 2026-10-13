//! VR 固有の操作シナリオ: コントローラ機種・左右の手・パネルの掴み移動。
//! 基本操作は home.rs で全ランタイム共通に検証する。

use gpui::TestAppContext;
use home::Home;
use home::{BUTTON_ACTIVE_BG, BUTTON_BG};
use testkit::{Action, Driver, Hand, Vec2, Vec3, VrKit};

#[gpui::test]
fn every_controller_can_click(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
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
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    kit.hand(Hand::Left)
        .profile("oculus/touch_controller")
        .click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn dragged_panel_moves_in_vr_and_stays_clickable(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    let to = Vec3::new(0.3, 0.2, -1.2);
    kit.drag_panel(to);
    assert!(kit.panel_pose().position.abs_diff_eq(to, 1e-4));
    assert!(kit.vr.overlays[0].pose.position.abs_diff_eq(to, 1e-4));

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

/// メニューボタンを押している間、Home の表示が光る。
#[gpui::test]
fn menu_button_shows_on_home(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    kit.hover("click-me").press(Action::Menu);
    assert_eq!(kit.color_at("menu-held"), BUTTON_ACTIVE_BG);
    kit.release(Action::Menu);
    assert_eq!(kit.color_at("menu-held"), BUTTON_BG);
}

/// パネルを掴んでいる間、Home の表示が光る。
#[gpui::test]
fn grabbing_panel_shows_on_home(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    let (w, h) = kit.overlay.panel(0).resolution;
    kit.point_at(Vec2::new(w as f32, h as f32) / 2.0)
        .press(Action::Grab);
    assert_eq!(kit.color_at("grabbed"), BUTTON_ACTIVE_BG);
    kit.release(Action::Grab);
    assert_eq!(kit.color_at("grabbed"), BUTTON_BG);
}
