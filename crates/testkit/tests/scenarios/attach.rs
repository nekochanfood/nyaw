//! VR-specific operation scenarios: panels attached to devices (HMD, controllers, trackers).
//! Moving the attach target makes the panel follow in VR, and it can still be operated with the laser.
//! Attaching to the HMD or hands, detaching, and locking are done by pressing Testing's buttons with the laser.

use gpui::TestAppContext;
use testing::{HAND_OFFSET, Testing};
use testkit::{Device, Driver, Hand, Pose, Quat, Vec3, VrKit};

#[gpui::test]
fn panel_on_left_hand_follows_it_and_right_hand_clicks(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    // Press "attach to left hand" with the right hand. The panel floats 0.3m in front of the left hand (like a wristwatch).
    let hand = Pose::new(Vec3::new(-0.4, 0.1, -0.5), Quat::IDENTITY);
    kit.click("attach-left").hand(Hand::Left).move_hand(hand);
    let expected = (hand * HAND_OFFSET).position;
    assert_eq!(expected, Vec3::new(-0.4, 0.1, -0.8));
    assert!(kit.panel_pose().position.abs_diff_eq(expected, 1e-4));
    assert!(
        kit.kernel.subsystems.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq(expected, 1e-4)
    );

    kit.hand(Hand::Right).click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn panel_on_hmd_follows_head_and_stays_clickable(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.click("attach-hmd"); // 1m straight in front of the face
    // Stand up and turn right.
    kit.kernel.subsystems.backend.frame.hmd = Pose::new(
        Vec3::new(0.0, 1.6, 0.0),
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
    );
    kit.step();
    let expected = Vec3::new(1.0, 1.6, 0.0);
    assert!(
        kit.kernel.subsystems.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq(expected, 1e-4)
    );

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn panel_on_tracker_follows_it(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    // Visible trackers appear as Testing buttons, so press that.
    kit.kernel.subsystems.backend.frame.trackers = vec![("LHR-WAIST".into(), Pose::IDENTITY)];
    kit.step().click("attach-tracker");
    let waist = Pose::new(Vec3::new(0.2, 0.9, 0.0), Quat::IDENTITY);
    kit.kernel.subsystems.backend.frame.trackers[0].1 = waist;
    kit.step();
    let expected = (waist * HAND_OFFSET).position;
    assert!(
        kit.kernel.subsystems.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq(expected, 1e-4)
    );
    assert_eq!(
        kit.kernel.panel(0).attached.as_ref().map(|a| &a.0),
        Some(&Device::Tracker("LHR-WAIST".into()))
    );

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn tracker_button_does_nothing_without_trackers(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.click("attach-tracker");
    assert_eq!(kit.kernel.panel(0).attached, None);
}

#[gpui::test]
fn panel_on_right_hand_is_operated_by_left_hand(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.hand(Hand::Left).click("attach-right");
    let hand = Pose::new(Vec3::new(0.3, 0.0, -0.4), Quat::IDENTITY);
    kit.hand(Hand::Right).move_hand(hand);
    assert!(
        kit.kernel.subsystems.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq((hand * HAND_OFFSET).position, 1e-4)
    );

    kit.hand(Hand::Left).click("detach");
    assert_eq!(kit.kernel.panel(0).attached, None);
}

#[gpui::test]
fn dragging_hmd_panel_keeps_new_offset(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.click("attach-hmd"); // 1m straight in front of the face
    let to = Vec3::new(0.3, 0.2, -1.2);
    kit.drag_panel(to);
    kit.kernel.subsystems.backend.frame.hmd = Pose::new(Vec3::new(0.0, 0.5, 0.0), Quat::IDENTITY);
    kit.step();
    assert!(
        kit.panel_pose()
            .position
            .abs_diff_eq(to + Vec3::new(0.0, 0.5, 0.0), 1e-4)
    );
}

#[gpui::test]
fn detached_and_locked_panel_stays_fixed_in_space(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.click("attach-hmd"); // 1m straight in front of the face
    kit.kernel.subsystems.backend.frame.hmd = Pose::new(Vec3::new(0.0, 0.3, 0.0), Quat::IDENTITY);
    kit.step();
    // Fix it in space at the current position: it does not move even if grabbed or if the head moves.
    let fixed = Vec3::new(0.0, 0.3, -1.0);
    kit.click("detach")
        .click("lock")
        .drag_panel(Vec3::new(0.5, 0.0, -0.5));
    kit.kernel.subsystems.backend.frame.hmd = Pose::new(Vec3::new(1.0, 0.3, 0.0), Quat::IDENTITY);
    kit.step();
    assert!(kit.panel_pose().position.abs_diff_eq(fixed, 1e-4));
    assert!(
        kit.kernel.subsystems.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq(fixed, 1e-4)
    );

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
