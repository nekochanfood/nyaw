use super::*;
use crate::test_util::*;

#[test]
fn in_view_estimates_from_head_direction() {
    let (mut vr, mut ov) = setup();
    ov.tick(&mut vr);
    assert!(ov.in_view(0), "1m straight ahead");
    // 真後ろを向くと見えない。
    vr.backend.frame.hmd = Pose::new(Vec3::ZERO, Quat::from_rotation_y(std::f32::consts::PI));
    ov.tick(&mut vr);
    assert!(!ov.in_view(0));
}

#[test]
fn attached_to_hmd_follows_head_including_rotation() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -0.5));
    vr.backend.frame.hmd = Pose::new(
        Vec3::new(1.0, 1.5, 0.0),
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    );
    ov.tick(&mut vr);
    // 左を向いた頭の 0.5m 前方 = -X 方向。
    assert_at(&ov, &vr, Vec3::new(0.5, 1.5, 0.0));
    assert!(
        ov.panel(0)
            .pose
            .orientation
            .abs_diff_eq(vr.backend.frame.hmd.orientation, 1e-5)
    );
}

#[test]
fn attached_to_controller_follows_hand() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Controller(Hand::Right), at(0.0, 0.1, 0.0));
    controller(&mut vr, at(0.3, -0.2, -0.4), &[]);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(0.3, -0.1, -0.4));
}

#[test]
fn attached_to_tracker_and_keeps_pose_when_lost() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Tracker("waist".into()), at(0.0, 0.0, -0.2));
    vr.backend.frame.trackers = vec![("waist".into(), at(0.0, 1.0, 0.0))];
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
    vr.backend.frame.trackers.clear();
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(0.0, 1.0, -0.2));
}

#[test]
fn detach_leaves_panel_in_place() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
    vr.backend.frame.hmd = at(2.0, 0.0, 0.0);
    ov.tick(&mut vr);
    ov.detach(0);
    vr.backend.frame.hmd = at(5.0, 0.0, 0.0);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(2.0, 0.0, -1.0));
}

#[test]
fn dragging_attached_panel_reanchors_offset() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
    // 右手で掴んで 1m 右へ動かして離す。
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    controller(&mut vr, at(1.0, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
    // 頭が動けば、新しいオフセットのまま追従する。
    vr.backend.frame.hmd = at(0.0, 0.5, 0.0);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(1.0, 0.5, -1.0));
}

#[test]
fn locked_panel_ignores_grab_but_still_takes_pointer() {
    let (mut vr, mut ov) = setup();
    ov.set_locked(0, true);
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move {
            pos: Vec2::new(50.0, 50.0)
        }]
    );
    controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
}

#[test]
fn lazy_following_panel_ignores_grab_but_still_takes_pointer() {
    let (mut vr, mut ov) = setup();
    ov.set_lazy_follow(0, Some(LazyFollow::new(1.0)));
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move {
            pos: Vec2::new(50.0, 50.0)
        }]
    );
    // 手を動かしても付いてこない（掴めない）。ゆるい追従が置いた、頭の正面のまま。
    controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    // 外せば掴める（いったん離して押し直す）。
    ov.set_lazy_follow(0, None);
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
}

#[test]
fn unlocked_panel_can_be_grabbed_again() {
    let (mut vr, mut ov) = setup();
    ov.set_locked(0, true);
    ov.set_locked(0, false);
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
}

#[test]
fn locked_attached_panel_still_follows_device() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
    ov.set_locked(0, true);
    vr.backend.frame.hmd = at(0.0, 0.5, 0.0);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(0.0, 0.5, -1.0));
}

#[test]
fn pixel_to_world_inverts_hit_test() {
    let (_, ov) = setup();
    let p = ov.panel(0);
    let px = Vec2::new(10.0, 70.0);
    let target = p.pixel_to_world(px);
    let aim = Pose::new(
        Vec3::ZERO,
        Quat::from_rotation_arc(Vec3::NEG_Z, target.normalize()),
    );
    let (hit, _) = hit_test(p, &aim).unwrap();
    assert!(hit.abs_diff_eq(px, 1e-3), "{hit}");
}
