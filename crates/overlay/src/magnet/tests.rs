use super::*;
use crate::test_util::*;

const GRAB: &[(&str, f32)] = &[("squeeze/value", 1.0)];

/// バー（0 番）の上端に、はめられる画面（1 番。0.4 × 0.3m）がはまっている組。画面は目の高さの正面 1m に立つ。
fn docked() -> (Subsystems<MockBackend>, Overlay) {
    let (mut vr, mut ov) = setup();
    // バーは低く（影の余白が画面の中心に重ならない）。
    ov.fit_height(&mut vr.backend, 0, 0.2);
    let screen = ov
        .add_panel(
            &mut vr.backend,
            "screen",
            at(0.0, 0.0, -1.0),
            Vec2::new(0.4, 0.3),
            (400, 300),
        )
        .unwrap();
    ov.set_animated(&mut vr.backend, screen, false);
    ov.set_resizable(0, false);
    ov.set_dock_slot(0, Some(0.0));
    ov.set_dockable(screen, true);
    ov.set_parent(&mut vr.backend, screen, Some(slot_parent()));
    ov.tick(&mut vr);
    let lift = ov.panel(screen).pose.position.y;
    ov.set_pose(0, at(0.0, -lift, -1.0));
    ov.tick(&mut vr);
    (vr, ov)
}

fn slot_parent() -> Parent {
    Parent {
        panel: 0,
        carry: true,
        docked: true,
        slot: Some(0.0),
        ..Default::default()
    }
}

/// 頭（原点）から `point` を狙う手。掴んだ点はレーザーに沿って同じ距離のまま付いてくる。
fn aim_at(point: Vec3) -> Pose {
    Pose::new(
        Vec3::ZERO,
        Quat::from_rotation_arc(Vec3::NEG_Z, point.normalize()),
    )
}

/// 掴んだまま `to` の方へ手を向け、落ち着くまで待つ。
fn pull_to(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay, to: Vec3) {
    controller(vr, aim_at(to), GRAB);
    settle(vr, ov);
}

/// 手を離して落ち着くまで待つ（向きはそのまま）。
fn release(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay, to: Vec3) {
    controller(vr, aim_at(to), &[]);
    settle(vr, ov);
}

#[test]
fn a_docked_screen_resists_a_pull_then_pops_out_into_the_hand() {
    let (mut vr, mut ov) = docked();
    let (slot, bar) = (ov.panel(1).pose, ov.panel(0).pose);
    controller(&mut vr, aim_at(slot.position), GRAB);
    ov.tick(&mut vr);
    assert!(ov.grabbed(1));
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Pull));
    // 少し引いても手ほどには付いてこない（磁石に引かれている）。引く向きへ少し傾き、ダッシュボードは動かない。
    pull_to(&mut vr, &mut ov, slot.position + Vec3::X * 0.1);
    let p = ov.panel(1);
    let moved = p.pose.position.distance(slot.position);
    assert!(moved > 0.01 && moved <= GIVE, "{moved}");
    assert!(p.pose.orientation.angle_between(slot.orientation) > 0.01);
    assert!(p.parent.is_some());
    assert_eq!(ov.panel(0).pose, bar);
    assert!(ov.take_dock_changes().is_empty());
    // 抜けるまで引くと、ダッシュボードから外れて手に付いて動く（ダッシュボードはその場に残る）。
    let to = slot.position + Vec3::X * 0.5;
    pull_to(&mut vr, &mut ov, to);
    assert!(ov.panel(1).parent.is_none());
    assert_eq!(ov.take_dock_changes(), vec![(1, false)]);
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Attach));
    // 手ブレ抑えでレーザーは少し遅れて付いてくる。
    let got = ov.panel(1).pose.position;
    assert!(got.abs_diff_eq(to.normalize(), 0.01), "{got}");
    assert_eq!(ov.panel(0).pose, bar);
    // 離しても外れたまま。
    release(&mut vr, &mut ov, to);
    assert!(ov.panel(1).parent.is_none());
    assert!(ov.take_dock_changes().is_empty());
}

#[test]
fn letting_go_before_it_pops_out_snaps_it_back() {
    let (mut vr, mut ov) = docked();
    let slot = ov.panel(1).pose;
    controller(&mut vr, aim_at(slot.position), GRAB);
    ov.tick(&mut vr);
    let to = slot.position + Vec3::new(0.08, 0.05, 0.0);
    pull_to(&mut vr, &mut ov, to);
    assert!(ov.panel(1).pose.position.distance(slot.position) > 0.01);
    release(&mut vr, &mut ov, to);
    let p = ov.panel(1);
    assert!(p.parent.is_some());
    assert!(p.pose.position.abs_diff_eq(slot.position, 1e-4));
    assert!(p.pose.orientation.angle_between(slot.orientation) < 1e-3);
    assert!(ov.take_dock_changes().is_empty());
}

#[test]
fn a_free_screen_is_drawn_in_catches_and_docks_when_let_go() {
    let (mut vr, mut ov) = docked();
    let slot = ov.panel(1).pose;
    ov.set_parent(&mut vr.backend, 1, None);
    let away = Vec3::new(0.6f32.sin(), 0.0, -0.6f32.cos());
    ov.set_pose(1, Pose::new(away, Quat::IDENTITY));
    ov.tick(&mut vr);
    controller(&mut vr, aim_at(away), GRAB);
    ov.tick(&mut vr);
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Attach));
    // 近づけていくと、手の位置よりはまる所へ引き寄せられる。
    let near = slot.position + Vec3::X * 0.2;
    pull_to(&mut vr, &mut ov, near);
    let hand = near.normalize();
    let got = ov.panel(1).pose.position;
    assert!(
        got.distance(slot.position) < hand.distance(slot.position) - 0.02,
        "{got}"
    );
    // さらに近づけると吸い付く（離すまでは、まだはまっていない）。
    let close = slot.position + Vec3::X * 0.05;
    pull_to(&mut vr, &mut ov, close);
    assert!(ov.panel(1).pose.position.distance(slot.position) < GIVE);
    assert!(ov.panel(1).parent.is_none());
    // 離すとはまる。
    release(&mut vr, &mut ov, close);
    let p = ov.panel(1);
    assert_eq!(p.parent, Some(slot_parent()));
    assert!(p.pose.position.abs_diff_eq(slot.position, 1e-4));
    assert_eq!(ov.take_dock_changes(), vec![(1, true)]);
}

#[test]
fn a_caught_screen_can_be_pulled_off_again_without_docking() {
    let (mut vr, mut ov) = docked();
    let slot = ov.panel(1).pose;
    ov.set_parent(&mut vr.backend, 1, None);
    let away = Vec3::new(0.6f32.sin(), 0.0, -0.6f32.cos());
    ov.set_pose(1, Pose::new(away, Quat::IDENTITY));
    ov.tick(&mut vr);
    controller(&mut vr, aim_at(away), GRAB);
    ov.tick(&mut vr);
    pull_to(&mut vr, &mut ov, slot.position + Vec3::X * 0.05);
    let far = slot.position - Vec3::X * 0.5;
    pull_to(&mut vr, &mut ov, far);
    release(&mut vr, &mut ov, far);
    assert!(ov.panel(1).parent.is_none());
    assert!(ov.take_dock_changes().is_empty());
}

#[test]
fn grabbing_the_bar_still_moves_the_whole_dashboard() {
    let (mut vr, mut ov) = docked();
    let bar = ov.panel(0).pose.position;
    controller(&mut vr, aim_at(bar), GRAB);
    ov.tick(&mut vr);
    assert_eq!(ov.dock_hint(Hand::Right), None);
    pull_to(&mut vr, &mut ov, bar + Vec3::X * 0.5);
    assert!(ov.panel(0).pose.position.distance(bar) > 0.3);
    assert!(ov.panel(1).parent.is_some());
    assert!(ov.take_dock_changes().is_empty());
}

#[test]
fn no_magnet_when_another_screen_is_docked_or_the_panel_cannot_dock() {
    let (mut vr, mut ov) = docked();
    let away = Vec3::new(0.6f32.sin(), 0.0, -0.6f32.cos());
    let other = ov
        .add_panel(
            &mut vr.backend,
            "other",
            Pose::new(away, Quat::IDENTITY),
            Vec2::new(0.4, 0.3),
            (400, 300),
        )
        .unwrap();
    ov.set_dockable(other, true);
    ov.tick(&mut vr);
    controller(&mut vr, aim_at(away), GRAB);
    ov.tick(&mut vr);
    assert!(ov.grabbed(other));
    assert_eq!(ov.dock_hint(Hand::Right), None);
    // 空いていても、はめられない画面には効かない。
    release(&mut vr, &mut ov, away);
    ov.set_parent(&mut vr.backend, 1, None);
    ov.set_dockable(other, false);
    controller(&mut vr, aim_at(away), GRAB);
    ov.tick(&mut vr);
    assert_eq!(ov.dock_hint(Hand::Right), None);
}

#[test]
fn the_ghost_shows_where_the_screen_docks_at_its_size_with_a_hint_band() {
    let (mut vr, mut ov) = docked();
    let ghost = ov
        .add_panel(
            &mut vr.backend,
            "ghost",
            Pose::IDENTITY,
            Vec2::splat(0.1),
            (64, 64),
        )
        .unwrap();
    ov.set_pointer(&mut vr.backend, ghost, Hand::Right, Pointer::Dock);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[ghost].visible);
    assert!(vr.backend.overlays[ghost].sort < vr.backend.overlays[1].sort);
    let slot = ov.panel(1).pose;
    controller(&mut vr, aim_at(slot.position), GRAB);
    ov.tick(&mut vr);
    pull_to(&mut vr, &mut ov, slot.position + Vec3::X * 0.5);
    // 抜けた瞬間は光って少し大きい。
    let margin = PANEL_MARGIN * 0.3 / 300.0;
    let face = Vec2::new(0.4, 0.3) - Vec2::splat(2.0 * margin);
    let o = &vr.backend.overlays[ghost];
    assert!(o.visible && o.alpha > 0.0);
    assert!(o.width_m > face.x + 1e-3, "{}", o.width_m);
    // 光が引くと、地（影の余白の内側）と同じ大きさで、地の部分がはまる所に重なる。帯の分だけ上に伸びる。
    std::thread::sleep(std::time::Duration::from_secs_f32(FLASH + 0.05));
    ov.tick(&mut vr);
    let o = &vr.backend.overlays[ghost];
    assert!((o.width_m - face.x).abs() < 1e-4, "{}", o.width_m);
    let res = ov.panel(ghost).resolution;
    assert_eq!(
        res,
        (
            (face.x * 1000.0).round() as u32,
            (face.y * 1000.0).round() as u32 + DOCK_HINT_PX
        )
    );
    let band = DOCK_HINT_PX as f32 / 1000.0;
    assert!(
        o.pose
            .position
            .abs_diff_eq(slot.position + Vec3::Y * band / 2.0, 1e-4)
    );
    // 離すと薄れて消える。
    release(&mut vr, &mut ov, slot.position + Vec3::X * 0.5);
    std::thread::sleep(std::time::Duration::from_millis(
        Motion::Exit.duration_ms() + 50,
    ));
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[ghost].visible);
}
