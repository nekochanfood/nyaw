//! Test panels for rendering: testing-anim changes on every frame, testing-lod shows the current LOD by number and color.

use gpui::TestAppContext;
use kernel::PanelCommand;
use testing::{LOD_COLORS, TestingAnim, TestingLod};
use testkit::{Driver, Pose, Quat, Vec3, VrKit};

#[gpui::test]
fn anim_panel_redraws_every_frame_without_input(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| TestingAnim::new(0, cx));
    let before = kit.read(|a| a.frames);
    let texture = kit.texture().clone();
    kit.step().step().step();
    assert!(kit.read(|a| a.frames) >= before + 3, "drawn on every step");
    assert_ne!(*kit.texture(), texture, "the picture changes");
}

#[gpui::test]
fn lod_panel_shows_level_by_color(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| TestingLod::new(0));
    // Mid switches at 2 / 4 / 8 m for a panel with the reference density of 1000 px/m.
    let panel = kit.kernel.panel(0);
    let scale = panel.size.x * 1000.0 / panel.resolution.0 as f32;
    for (relative_distance, lod) in [(1.0, 0), (3.0, 1), (5.0, 2), (9.0, 3)] {
        let distance = relative_distance * scale;
        kit.kernel.apply(
            0,
            PanelCommand::Place(Pose::new(Vec3::new(0.0, 0.0, -distance), Quat::IDENTITY)),
        );
        kit.step();
        assert_eq!(kit.kernel.status(0).lod, lod, "{distance}m");
        assert_eq!(
            kit.color_at("lod-level"),
            LOD_COLORS[lod as usize],
            "{distance}m"
        );
    }
}
