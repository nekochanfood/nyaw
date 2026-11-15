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

/// 範囲（`reach`）を決めた取り付け中のパネルは、掴んで遠くへ動かしても取り付け先からその距離で止まり、そこで付け直される。
#[test]
fn reach_keeps_a_dragged_attached_panel_near_its_device() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
    ov.set_reach(0, Some(1.5));
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(3.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    let toward = Vec3::new(3.0, 0.0, -1.0).normalize() * 1.5;
    assert_at(&ov, &vr, toward);
    controller(&mut vr, at(3.0, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    vr.backend.frame.hmd = at(0.0, 0.5, 0.0);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, toward + Vec3::Y * 0.5);
}

/// 掴んでいる最中にロックすると掴みは取り消され、取り付け中のパネルは掴む前のオフセットへ戻る（離しても付け直さない）。
#[test]
fn locking_mid_grab_undoes_the_grab_of_an_attached_panel() {
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    assert_at(&ov, &vr, Vec3::new(1.0, 0.0, -1.0));
    ov.set_locked(&mut vr.backend, 0, true);
    assert!(!ov.grabbed(0));
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    controller(&mut vr, at(1.0, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    vr.backend.frame.hmd = at(0.0, 0.5, 0.0);
    ov.tick(&mut vr);
    assert_at(&ov, &vr, Vec3::new(0.0, 0.5, -1.0));
}

/// 手に持ったパネルは、レーザーの出る所の前へ伸び（上から見るとコントローラの先に面がある）、手前へ `HAND_TILT` 起きる。
/// 地の下端の左右中央は、面に沿ってレーザーの出る所から `HAND_GAP` の所。大きければ長い辺が `HAND_SIZE` になるまで縮む。
#[test]
fn panel_in_hand_lies_ahead_of_the_laser_tilted_toward_the_user() {
    let (mut vr, mut ov) = setup();
    ov.attach_to_hand(&mut vr.backend, 0, Hand::Right);
    assert!(matches!(
        ov.panel(0).attached,
        Some((Device::Controller(Hand::Right), _))
    ));
    assert!(ov.panel(0).size.abs_diff_eq(Vec2::splat(HAND_SIZE), 1e-6));
    assert!((vr.backend.overlays[0].width_m - HAND_SIZE).abs() < 1e-5);
    let face = ov.panel(0).size.y / 2.0 - face_margin(ov.panel(0));
    let hand = Vec3::new(0.2, -0.3, -0.4);
    let (sin, cos) = HAND_TILT.sin_cos();
    let eq = |a: Vec3, b: Vec3| assert!(a.abs_diff_eq(b, 1e-5), "{a} != {b}");
    // 水平に前へ向けた手: 面は前へ伸び、上（前）の端が手前へ起きて、表は上やや手前を向く。
    controller(&mut vr, Pose::new(hand, Quat::IDENTITY), &[]);
    ov.tick(&mut vr);
    let p = ov.panel(0).pose;
    let up = Vec3::new(0.0, sin, -cos);
    eq(p.orientation * Vec3::Y, up);
    eq(p.orientation * Vec3::Z, Vec3::new(0.0, cos, sin));
    eq(p.position - up * face, hand + up * HAND_GAP);
    // 手を起こす（レーザーが真上）と、面は立って上の端が手前に傾き、表が手前（+Z）を向いて正立する。
    let raised = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    controller(&mut vr, Pose::new(hand, raised), &[]);
    ov.tick(&mut vr);
    let p = ov.panel(0).pose;
    let up = Vec3::new(0.0, cos, sin);
    eq(p.orientation * Vec3::Y, up);
    eq(p.orientation * Vec3::Z, Vec3::new(0.0, -sin, cos));
    eq(p.orientation * Vec3::X, Vec3::X);
    eq(p.position - up * face, hand + up * HAND_GAP);
}

/// もともと小さいパネルは、手に持たせても大きさを変えない。
#[test]
fn small_panel_keeps_its_size_in_hand() {
    let (mut vr, mut ov) = setup();
    let small = Vec2::new(0.3, 0.2);
    let id = ov
        .add_panel(&mut vr.backend, "small", Pose::IDENTITY, small, (300, 200))
        .unwrap();
    ov.attach_to_hand(&mut vr.backend, id, Hand::Left);
    assert_eq!(ov.panel(id).size, small);
}

/// 掴んで大きさを変えている最中にロックすると、大きさも掴む前に戻る（手に持ったパネルの調整の取り消し）。
#[test]
fn locking_mid_grab_undoes_the_resize() {
    const RESIZE: &str = subsystems::vr::RESIZE_INPUT;
    let (mut vr, mut ov) = setup();
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
    let before = ov.panel(0).size;
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(
        &mut vr,
        Pose::IDENTITY,
        &[("squeeze/value", 1.0), (RESIZE, 1.0)],
    );
    settle(&mut vr, &mut ov);
    assert!(ov.panel(0).size.x > before.x * 1.5);
    ov.set_locked(&mut vr.backend, 0, true);
    assert_eq!(ov.panel(0).size, before);
    let width = vr.backend.overlays[0].width_m;
    assert!((width - before.x).abs() < 1e-5, "{width}");
    ov.tick(&mut vr);
    assert_eq!(ov.panel(0).size, before);
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
}

#[test]
fn locked_panel_ignores_grab_but_still_takes_pointer() {
    let (mut vr, mut ov) = setup();
    ov.set_locked(&mut vr.backend, 0, true);
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
    ov.set_locked(&mut vr.backend, 0, true);
    ov.set_locked(&mut vr.backend, 0, false);
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
    ov.set_locked(&mut vr.backend, 0, true);
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
