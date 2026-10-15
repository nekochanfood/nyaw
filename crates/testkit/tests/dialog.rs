//! Parent/child panels on the Dialog test screen: the dialog follows its parent, the parent dims while the dialog is open,
//! and grabbing the dialog (which is built not to carry its parent) leaves the parent behind.

use gpui::TestAppContext;
use kernel::PanelCommand;
use testing::{BUTTON_BG, DialogTest};
use testkit::{Driver, Pose, Quat, Vec3, VrKit};

fn channels(c: u32) -> [u32; 3] {
    [c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff]
}

#[gpui::test]
fn dialog_dims_its_parent_follows_it_and_leaves_alone_when_grabbed(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| DialogTest::new(0));
    assert_eq!(kit.color_at("open-dialog"), BUTTON_BG);
    kit.click("open-dialog").step().step();
    let dialog = 1;
    assert_eq!(kit.kernel.status(dialog).parent, Some(0));

    // The parent is dimmed (the color is darker, not more transparent) while the dialog is open.
    let dimmed = kit.panel(0).color_at("open-dialog");
    assert!(
        channels(dimmed)
            .iter()
            .zip(channels(BUTTON_BG))
            .all(|(d, b)| *d < b),
        "{dimmed:06x}"
    );

    // Grabbing the dialog moves only the dialog.
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
    assert_eq!(kit.kernel.panel(0).pose, parent_at);

    // Moving the parent takes the dialog along, keeping where it was left.
    let apart = kit.kernel.panel(dialog).pose.position - parent_at.position;
    kit.kernel.apply(
        0,
        PanelCommand::Place(Pose::new(Vec3::new(1.0, 0.0, -2.0), Quat::IDENTITY)),
    );
    kit.step();
    let parent = kit.kernel.panel(0).pose.position;
    assert!(parent.distance(parent_at.position) > 0.5);
    assert!(
        kit.kernel
            .panel(dialog)
            .pose
            .position
            .abs_diff_eq(parent + apart, 1e-3)
    );

    // Once the dialog is closed the parent is bright again.
    kit.kernel.apply(dialog, PanelCommand::Close);
    kit.panel(0).hover("dialog-answer").step();
    assert_eq!(kit.color_at("open-dialog"), BUTTON_BG);
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
