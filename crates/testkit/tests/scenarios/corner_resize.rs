//! Resizing a panel by a corner (the testing screen's `resize-test`): the panel resolution, and with it the view's viewport,
//! follows the hand on the moved axis only, both sides at once, within the limits.

use gpui::TestAppContext;
use kernel::{PanelCommand, ResizeLimits};
use testing::ResizeTest;
use testkit::{Action, Vec2, Vec3, VrKit};

const LIMITS: ResizeLimits = ResizeLimits {
    min: (200, 200),
    max: (8000, 8000),
    keep_aspect: false,
};

/// Width of the `resize-size` line, which spans the viewport less the margin and the padding.
fn line_width(kit: &mut VrKit<'_, ResizeTest>) -> f32 {
    kit.bounds("resize-size").size.width.into()
}

#[gpui::test]
fn grabbing_a_corner_changes_the_viewport_on_the_moved_axis(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| ResizeTest);
    kit.kernel
        .apply(0, PanelCommand::CornerResize(Some(LIMITS)));
    let (w, h) = kit.kernel.panel(0).resolution;
    let density = w as f32 / kit.kernel.panel(0).size.x;
    let center = kit.panel_pose().position;
    let before = line_width(&mut kit);

    // Grab near the bottom-right corner and move the hand 10cm to the left.
    let grab = Vec2::new(w as f32 - 40.0, h as f32 - 40.0);
    let to = kit.kernel.panel(0).pixel_to_world(grab) - Vec3::X * 0.1;
    kit.point_at(grab).press(Action::Grab);
    kit.aim_at(to).settle();

    // Both the left and the right edge come in by 10cm; the height and the center stay.
    let (w1, h1) = kit.kernel.panel(0).resolution;
    let narrower = w - w1;
    assert!((narrower as f32 - 0.2 * density).abs() <= 1.0, "{w} → {w1}");
    assert_eq!(h1, h);
    assert!(kit.panel_pose().position.abs_diff_eq(center, 1e-4));
    // The view is laid out again in the narrower viewport, and that is what VR shows.
    let line = line_width(&mut kit);
    assert!(
        (line - (before - narrower as f32)).abs() < 0.5,
        "{before} → {line}"
    );
    let tex = kit.texture();
    let shown = tex.width as f32 / tex.height as f32;
    assert!((shown - w1 as f32 / h as f32).abs() < 0.01, "{shown}");

    // Released, the size stays; moving the hand afterwards changes nothing.
    kit.release(Action::Grab);
    kit.aim_at(to + Vec3::X * 0.2).settle();
    assert_eq!(kit.kernel.panel(0).resolution, (w1, h1));
}

#[gpui::test]
fn a_panel_without_the_option_moves_when_grabbed_at_a_corner(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| ResizeTest);
    let (w, h) = kit.kernel.panel(0).resolution;
    let center = kit.panel_pose().position;
    let grab = Vec2::new(w as f32 - 40.0, h as f32 - 40.0);
    let to = kit.kernel.panel(0).pixel_to_world(grab) - Vec3::X * 0.1;
    kit.point_at(grab).press(Action::Grab);
    kit.aim_at(to).settle();
    assert_eq!(kit.kernel.panel(0).resolution, (w, h));
    assert!(kit.panel_pose().position.distance(center) > 0.05);
}
