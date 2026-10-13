//! オーバーレイ ⇄ UI の往復: レーザー入力が UI に届き、その描画結果が VR パネルに映ること。

use gpui::TestAppContext;
use testkit::{Kit, Vec2, Vec3};
use ui::{BUTTON_BG, BUTTON_HOVER_BG, Home, PANEL_BG};

#[gpui::test]
fn vr_panel_shows_rendered_ui(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    assert_eq!(kit.vr_color_at("click-me"), BUTTON_BG);
    assert_eq!(kit.vr_color_at_px(Vec2::new(2.0, 2.0)), PANEL_BG);

    let res = kit.overlay.panel(0).resolution;
    let tex = kit.texture();
    assert!(
        tex.width >= res.0 && tex.height >= res.1,
        "HiDPI でも論理解像度以上"
    );
}

#[gpui::test]
fn laser_hover_highlight_reaches_vr(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    kit.hover("click-me");
    assert_eq!(kit.vr_color_at("click-me"), BUTTON_HOVER_BG);

    kit.point_at(Vec2::new(2.0, 2.0));
    assert_eq!(kit.vr_color_at("click-me"), BUTTON_BG);
}

#[gpui::test]
fn moved_panel_keeps_receiving_frames(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    kit.drag_panel(Vec3::new(-0.4, 0.1, -1.5)).hover("click-me");
    assert_eq!(kit.vr_color_at("click-me"), BUTTON_HOVER_BG);
}
