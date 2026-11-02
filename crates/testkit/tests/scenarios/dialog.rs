//! Parent/child panels on the Dialog test screen: the dialog follows its parent, the parent dims while the dialog is open,
//! and grabbing the dialog (which is built to carry its parent) takes the parent along.

use gpui::TestAppContext;
use kernel::PanelCommand;
use testing::{BUTTON_BG, DialogTest};
use testkit::{Action, Driver, Pose, Quat, Vec2, Vec3, VrKit};

#[gpui::test]
fn dialog_dims_its_parent_follows_it_and_carries_it_when_grabbed(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| DialogTest::new(0));
    assert_eq!(kit.color_at("open-dialog"), BUTTON_BG);
    kit.click("open-dialog").step().step();
    let dialog = 1;
    assert_eq!(kit.kernel.status(dialog).parent, Some(0));

    // The parent is dimmed by the runtime (only the color, and the pixels sent are untouched) while the dialog is open.
    let tint = |kit: &VrKit<'_, DialogTest>| kit.kernel.subsystems.backend.overlays[0].tint;
    assert!(tint(&kit) < 1.0);

    // Grabbing the dialog takes the parent along, keeping how they sit relative to each other.
    let parent_at = kit.kernel.panel(0).pose;
    let target = kit.kernel.panel(dialog).pose.position + Vec3::new(0.3, 0.2, 0.0);
    kit.panel(dialog).drag_panel(target);
    assert!(
        kit.kernel
            .panel(dialog)
            .pose
            .position
            .abs_diff_eq(target, 1e-3)
    );
    let moved = kit.kernel.panel(0).pose.position;
    assert!(moved.distance(parent_at.position) > 0.2, "{moved:?}");

    // Moving the parent takes the dialog along.
    let apart = kit.kernel.panel(0).pose.inverse() * kit.kernel.panel(dialog).pose;
    kit.kernel.apply(
        0,
        PanelCommand::Place(Pose::new(Vec3::new(1.0, 0.0, -2.0), Quat::IDENTITY)),
    );
    kit.step();
    let parent = kit.kernel.panel(0).pose;
    assert!(parent.position.distance(moved) > 0.5);
    assert!(
        kit.kernel
            .panel(dialog)
            .pose
            .position
            .abs_diff_eq((parent * apart).position, 1e-3)
    );

    // Once the dialog is closed the parent is bright again.
    kit.kernel.apply(dialog, PanelCommand::Close);
    kit.panel(0).hover("dialog-answer").step();
    assert_eq!(kit.color_at("open-dialog"), BUTTON_BG);
    assert_eq!(tint(&kit), 1.0);
}

#[gpui::test]
fn answering_closes_the_dialog_and_reports_to_the_parent(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| DialogTest::new(0));
    kit.click("open-dialog").step().step();
    kit.panel(1).click("dialog-ok").step().step();
    assert!(kit.kernel.panel(1).closed);
    kit.panel(0).hover("dialog-answer").step();
    assert_eq!(kit.read(|t| (t.answered, t.answer)), (1, Some("OK")));
    assert_eq!(kit.color_at("open-dialog"), BUTTON_BG);
}

/// Presses and releases the select button on empty space, off the edge of the panels.
fn click_outside(kit: &mut VrKit<'_, DialogTest>) {
    kit.panel(0)
        .point_at(Vec2::new(-400.0, 100.0))
        .press(Action::Select)
        .release(Action::Select)
        .step()
        .step();
}

#[gpui::test]
fn pressing_outside_closes_the_dialog_unless_it_is_the_modal_one(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| DialogTest::new(0));
    let tint = |kit: &VrKit<'_, DialogTest>| kit.kernel.subsystems.backend.overlays[0].tint;
    // The ordinary dialog goes away, and the parent is bright again.
    kit.click("open-dialog").step().step();
    assert!(tint(&kit) < 1.0);
    click_outside(&mut kit);
    assert!(kit.kernel.panel(1).closed);
    assert_eq!(tint(&kit), 1.0);

    // The modal one stays until it is answered.
    kit.click("open-modal-dialog").step().step();
    let modal = 2;
    click_outside(&mut kit);
    assert!(!kit.kernel.panel(modal).closed);
    assert!(tint(&kit) < 1.0);
    kit.panel(modal).click("dialog-ok").step().step();
    assert!(kit.kernel.panel(modal).closed);
}
