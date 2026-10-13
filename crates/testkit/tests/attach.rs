//! VR 固有の操作シナリオ: デバイス（HMD・コントローラ・トラッカー）に取り付けたパネル。
//! 取り付け先を動かすとパネルが VR 上で追従し、そのままレーザーで操作できること。
//! HMD・手への取り付け・取り外し・ロックは Testing のボタンをレーザーで押して操作する。

use gpui::TestAppContext;
use testing::{HAND_OFFSET, Testing};
use testkit::{Device, Driver, Hand, Pose, Quat, Vec3, VrKit};

#[gpui::test]
fn panel_on_left_hand_follows_it_and_right_hand_clicks(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    // 右手で「左手に取り付け」を押す。パネルは左手の 0.3m 前に浮かぶ（手首の時計のような配置）。
    let hand = Pose::new(Vec3::new(-0.4, 0.1, -0.5), Quat::IDENTITY);
    kit.click("attach-left").hand(Hand::Left).move_hand(hand);
    let expected = (hand * HAND_OFFSET).position;
    assert_eq!(expected, Vec3::new(-0.4, 0.1, -0.8));
    assert!(kit.panel_pose().position.abs_diff_eq(expected, 1e-4));
    assert!(
        kit.kernel.vr.overlays[0]
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
    kit.click("attach-hmd"); // 顔の正面 1m
    // 立ち上がって右を向く。
    kit.kernel.vr.frame.hmd = Pose::new(
        Vec3::new(0.0, 1.6, 0.0),
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
    );
    kit.step();
    let expected = Vec3::new(1.0, 1.6, 0.0);
    assert!(
        kit.kernel.vr.overlays[0]
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
    // 見えているトラッカーが Testing のボタンに出るので、それを押す。
    kit.kernel.vr.frame.trackers = vec![("LHR-WAIST".into(), Pose::IDENTITY)];
    kit.step().click("attach-tracker");
    let waist = Pose::new(Vec3::new(0.2, 0.9, 0.0), Quat::IDENTITY);
    kit.kernel.vr.frame.trackers[0].1 = waist;
    kit.step();
    let expected = (waist * HAND_OFFSET).position;
    assert!(
        kit.kernel.vr.overlays[0]
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
        kit.kernel.vr.overlays[0]
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
    kit.click("attach-hmd"); // 顔の正面 1m
    let to = Vec3::new(0.3, 0.2, -1.2);
    kit.drag_panel(to);
    kit.kernel.vr.frame.hmd = Pose::new(Vec3::new(0.0, 0.5, 0.0), Quat::IDENTITY);
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
    kit.click("attach-hmd"); // 顔の正面 1m
    kit.kernel.vr.frame.hmd = Pose::new(Vec3::new(0.0, 0.3, 0.0), Quat::IDENTITY);
    kit.step();
    // 今の位置で空間に固定する: 掴んでも、頭を動かしても動かない。
    let fixed = Vec3::new(0.0, 0.3, -1.0);
    kit.click("detach")
        .click("lock")
        .drag_panel(Vec3::new(0.5, 0.0, -0.5));
    kit.kernel.vr.frame.hmd = Pose::new(Vec3::new(1.0, 0.3, 0.0), Quat::IDENTITY);
    kit.step();
    assert!(kit.panel_pose().position.abs_diff_eq(fixed, 1e-4));
    assert!(
        kit.kernel.vr.overlays[0]
            .pose
            .position
            .abs_diff_eq(fixed, 1e-4)
    );

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
