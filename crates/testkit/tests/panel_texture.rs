//! VR-specific: the overlay <-> UI round trip. Laser input reaches the UI, and its rendering result appears on the VR panel.

use gpui::TestAppContext;
use testing::{BUTTON_BG, BUTTON_HOVER_BG, PANEL_BG, Testing};
use testkit::{Driver, Vec2, Vec3, VrKit};

#[gpui::test]
fn vr_panel_shows_rendered_ui(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    assert_eq!(kit.color_at("click-me"), BUTTON_BG);
    // 角は角丸で透明なので、左の縁（地の余白）を見る。
    assert_eq!(kit.vr_color_at_px(Vec2::new(4.0, 60.0)), PANEL_BG);

    let res = kit.kernel.panel(0).resolution;
    let tex = kit.texture();
    assert!(
        tex.width >= res.0 && tex.height >= res.1,
        "at least the logical resolution even on HiDPI"
    );
}

#[gpui::test]
fn laser_hover_highlight_reaches_vr(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.hover("click-me");
    assert_eq!(kit.color_at("click-me"), BUTTON_HOVER_BG);

    kit.point_at(Vec2::new(2.0, 2.0));
    assert_eq!(kit.color_at("click-me"), BUTTON_BG);
}

#[gpui::test]
fn moved_panel_keeps_receiving_frames(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.drag_panel(Vec3::new(-0.4, 0.1, -1.5)).hover("click-me");
    assert_eq!(kit.color_at("click-me"), BUTTON_HOVER_BG);
}
