//! VR 固有の操作シナリオ: デバイス（HMD・コントローラ・トラッカー）に取り付けたパネル。
//! 取り付け先を動かすとパネルが VR 上で追従し、そのままレーザーで操作できること。

use gpui::TestAppContext;
use testkit::{Device, Driver, Hand, Pose, Quat, Vec3, VrKit};
use ui::Home;

#[gpui::test]
fn panel_on_left_hand_follows_it_and_right_hand_clicks(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    // 左手の 0.3m 前に浮かべる（手首の時計のような配置）。
    let offset = Pose::new(Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY);
    kit.hand(Hand::Left)
        .attach_panel(Device::Controller(Hand::Left), offset)
        .move_hand(Pose::new(Vec3::new(-0.4, 0.1, -0.5), Quat::IDENTITY));
    let expected = Vec3::new(-0.4, 0.1, -0.8);
    assert!(kit.panel_pose().position.abs_diff_eq(expected, 1e-4));
    assert!(kit.vr.overlays[0].pose.position.abs_diff_eq(expected, 1e-4));

    kit.hand(Hand::Right).click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn panel_on_hmd_follows_head_and_stays_clickable(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    kit.attach_panel(
        Device::Hmd,
        Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
    );
    // 立ち上がって右を向く。
    kit.vr.frame.hmd = Pose::new(
        Vec3::new(0.0, 1.6, 0.0),
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
    );
    kit.step();
    let expected = Vec3::new(1.0, 1.6, 0.0);
    assert!(kit.vr.overlays[0].pose.position.abs_diff_eq(expected, 1e-4));

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn panel_on_tracker_follows_it(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    let waist = Device::Tracker("LHR-WAIST".into());
    kit.vr.frame.trackers = vec![("LHR-WAIST".into(), Pose::IDENTITY)];
    kit.attach_panel(waist, Pose::new(Vec3::new(0.0, 0.0, -0.5), Quat::IDENTITY));
    kit.vr.frame.trackers[0].1 = Pose::new(Vec3::new(0.2, 0.9, 0.0), Quat::IDENTITY);
    kit.step();
    assert!(
        kit.vr.overlays[0]
            .pose
            .position
            .abs_diff_eq(Vec3::new(0.2, 0.9, -0.5), 1e-4)
    );

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn dragging_hmd_panel_keeps_new_offset(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    kit.attach_panel(
        Device::Hmd,
        Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
    );
    let to = Vec3::new(0.3, 0.2, -1.2);
    kit.drag_panel(to);
    kit.vr.frame.hmd = Pose::new(Vec3::new(0.0, 0.5, 0.0), Quat::IDENTITY);
    kit.step();
    assert!(
        kit.panel_pose()
            .position
            .abs_diff_eq(to + Vec3::new(0.0, 0.5, 0.0), 1e-4)
    );
}

#[gpui::test]
fn detached_and_locked_panel_stays_fixed_in_space(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    kit.attach_panel(
        Device::Hmd,
        Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
    );
    kit.vr.frame.hmd = Pose::new(Vec3::new(0.0, 0.3, 0.0), Quat::IDENTITY);
    kit.step();
    // 今の位置で空間に固定する: 掴んでも、頭を動かしても動かない。
    let fixed = Vec3::new(0.0, 0.3, -1.0);
    kit.detach_panel()
        .lock_panel(true)
        .drag_panel(Vec3::new(0.5, 0.0, -0.5));
    kit.vr.frame.hmd = Pose::new(Vec3::new(1.0, 0.3, 0.0), Quat::IDENTITY);
    kit.step();
    assert!(kit.panel_pose().position.abs_diff_eq(fixed, 1e-4));
    assert!(kit.vr.overlays[0].pose.position.abs_diff_eq(fixed, 1e-4));

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
