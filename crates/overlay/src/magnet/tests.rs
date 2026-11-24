use super::*;
use crate::test_util::*;

const GRAB: &[(&str, f32)] = &[("squeeze/value", 1.0)];

/// バー（0 番）の上端に、はめられる画面（1 番。0.4 × 0.3m）がはまっている組。画面は目の高さの正面 1m に立つ。
/// `nav_bar` なら、画面にナビゲーションバー（2 番）を付ける。
fn docked(nav_bar: bool) -> (Subsystems<MockBackend>, Overlay) {
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
    if nav_bar {
        let bar = ov
            .add_panel(
                &mut vr.backend,
                "nav",
                Pose::IDENTITY,
                Vec2::new(0.2, 0.03),
                (200, 30),
            )
            .unwrap();
        ov.set_nav_bar(&mut vr.backend, bar, screen);
    }
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

/// 画面（1 番）を外して、はまる所から横へ離れた所（頭から 1m）に置く。その場所を返す。
fn undock_aside(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay) -> Vec3 {
    ov.set_parent(&mut vr.backend, 1, None);
    let away = Vec3::new(0.6f32.sin(), 0.0, -0.6f32.cos());
    ov.set_pose(1, Pose::new(away, Quat::IDENTITY));
    ov.tick(vr);
    away
}

#[test]
fn a_docked_screen_holds_still_then_pops_out_into_the_hand() {
    let (mut vr, mut ov) = docked(false);
    let (slot, bar) = (ov.panel(1).pose, ov.panel(0).pose);
    controller(&mut vr, aim_at(slot.position), GRAB);
    ov.tick(&mut vr);
    assert!(ov.grabbed(1));
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Pull));
    // 抜けるまでは、はまる所から動かない（位置も向きも大きさも）。ダッシュボードも動かない。
    pull_to(&mut vr, &mut ov, slot.position + Vec3::new(0.1, 0.05, 0.0));
    let p = ov.panel(1);
    assert_eq!(p.pose, slot);
    assert_eq!(p.displayed_size(), p.size);
    assert!(p.parent.is_some());
    assert_eq!(ov.panel(0).pose, bar);
    assert!(ov.take_dock_changes().is_empty());
    assert!(ov.take_dock_snaps().is_empty());
    // 抜けるまで引くと、ダッシュボードから外れて手に付いて動く（ダッシュボードはその場に残る）。
    let to = slot.position + Vec3::X * 0.5;
    pull_to(&mut vr, &mut ov, to);
    assert!(ov.panel(1).parent.is_none());
    assert_eq!(ov.take_dock_changes(), vec![(1, false)]);
    assert_eq!(ov.take_dock_snaps(), vec![(1, false)]);
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
fn letting_go_before_it_pops_out_leaves_it_docked() {
    let (mut vr, mut ov) = docked(false);
    let slot = ov.panel(1).pose;
    controller(&mut vr, aim_at(slot.position), GRAB);
    ov.tick(&mut vr);
    let to = slot.position + Vec3::new(0.08, 0.05, 0.0);
    pull_to(&mut vr, &mut ov, to);
    release(&mut vr, &mut ov, to);
    let p = ov.panel(1);
    assert!(p.parent.is_some());
    assert_eq!(p.pose, slot);
    assert!(ov.take_dock_changes().is_empty());
}

#[test]
fn resizing_a_docked_screen_does_not_pull_it_out() {
    let (mut vr, mut ov) = docked(false);
    let slot = ov.panel(1).pose;
    let resize = [GRAB[0], ("trigger/value", 1.0)];
    controller(&mut vr, aim_at(slot.position), GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim_at(slot.position), &resize);
    ov.tick(&mut vr);
    // トリガーを押したまま手を前に出して大きさを変えている間は、抜ける距離より大きく手を横に振っても抜けない。
    let size = ov.panel(1).size;
    let to = slot.position + Vec3::X * 0.5;
    let aim = aim_at(to);
    controller(
        &mut vr,
        Pose::new(aim.forward() * 0.05, aim.orientation),
        &resize,
    );
    settle(&mut vr, &mut ov);
    assert!(ov.panel(1).size.x > size.x);
    assert!(ov.panel(1).parent.is_some());
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Pull));
    // トリガーを離しただけでは抜けない。
    pull_to(&mut vr, &mut ov, to);
    assert!(ov.panel(1).parent.is_some());
    assert!(ov.take_dock_changes().is_empty());
    // そこからさらに引くと抜ける。
    pull_to(&mut vr, &mut ov, to + Vec3::X * 0.5);
    assert!(ov.panel(1).parent.is_none());
    assert_eq!(ov.take_dock_changes(), vec![(1, false)]);
}

#[test]
fn a_free_screen_is_drawn_in_catches_and_docks_when_let_go() {
    let (mut vr, mut ov) = docked(false);
    let slot = ov.panel(1).pose;
    let away = undock_aside(&mut vr, &mut ov);
    controller(&mut vr, aim_at(away), GRAB);
    ov.tick(&mut vr);
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Attach));
    // 近づけていくと、手の位置よりはまる所へ引き寄せられる（まだ離せば独立したまま）。
    let near = slot.position + Vec3::X * 0.2;
    pull_to(&mut vr, &mut ov, near);
    let hand = near.normalize();
    let got = ov.panel(1).pose.position;
    assert!(
        got.distance(slot.position) < hand.distance(slot.position) - 0.02,
        "{got}"
    );
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Attach));
    // さらに近づけると吸い付いてはまる所に収まり、離せばはまると分かる一行になる（離すまでは、まだはまっていない）。
    let close = slot.position + Vec3::X * 0.05;
    pull_to(&mut vr, &mut ov, close);
    assert!(ov.panel(1).pose.position.abs_diff_eq(slot.position, 1e-4));
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Release));
    assert!(DockHint::Release.docks() && !DockHint::Attach.docks());
    assert!(ov.panel(1).parent.is_none());
    assert!(ov.take_dock_changes().is_empty());
    assert_eq!(ov.take_dock_snaps(), vec![(1, true)]);
    // 離すとはまる（吸い付いたのは離す前なので、吸い付きとしては入らない）。
    release(&mut vr, &mut ov, close);
    let p = ov.panel(1);
    assert_eq!(p.parent, Some(slot_parent()));
    assert!(p.pose.position.abs_diff_eq(slot.position, 1e-4));
    assert_eq!(ov.take_dock_changes(), vec![(1, true)]);
    assert!(ov.take_dock_snaps().is_empty());
}

/// ダッシュボードから呼び出してはめる（バーやナビゲーションバーのボタン。[`Overlay::set_parent`]）ときも、手ではめるときと
/// 同じく、はまる所を少し行き過ぎてから収まる（[`Motion::Pop`]）。
#[test]
fn docking_from_the_dashboard_overshoots_and_settles() {
    let (mut vr, mut ov) = docked(false);
    let slot = ov.panel(1).pose.position;
    ov.set_animated(&mut vr.backend, 1, true);
    ov.submit(&mut vr.backend, 1, 1, 1, &[255; 4]);
    ov.set_parent(&mut vr.backend, 1, None);
    ov.glides.clear();
    ov.set_pose(1, at(0.6, 0.0, -1.0));
    let toward = (slot - ov.panel(1).pose.position).normalize();
    ov.set_parent(&mut vr.backend, 1, Some(slot_parent()));
    // 動きの時間を 20ms ずつ進め、はまる所へ向かう向きにどれだけ行き過ぎたかを測る。最後ははまる所に着く。
    let mut past = 0.0f32;
    for _ in 0..30 {
        if let Some(g) = ov.glides.get_mut(&1) {
            g.last -= std::time::Duration::from_millis(20);
        }
        ov.settle_family(&[], &[]);
        ov.glide(&[], Instant::now());
        past = past.max((ov.panel(1).pose.position - slot).dot(toward));
    }
    assert!(past > 0.01, "{past}");
    let last = ov.panel(1).pose.position.distance(slot);
    assert!(last < 1e-4, "{last}");
}

#[test]
fn a_caught_screen_can_be_pulled_off_again_without_docking() {
    let (mut vr, mut ov) = docked(false);
    let slot = ov.panel(1).pose;
    let away = undock_aside(&mut vr, &mut ov);
    controller(&mut vr, aim_at(away), GRAB);
    ov.tick(&mut vr);
    pull_to(&mut vr, &mut ov, slot.position + Vec3::X * 0.05);
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Release));
    assert_eq!(ov.take_dock_snaps(), vec![(1, true)]);
    // 親は変わっていなくても、抜けたことは入る。
    let far = slot.position - Vec3::X * 0.5;
    pull_to(&mut vr, &mut ov, far);
    assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Attach));
    assert_eq!(ov.take_dock_snaps(), vec![(1, false)]);
    release(&mut vr, &mut ov, far);
    assert!(ov.panel(1).parent.is_none());
    assert!(ov.take_dock_changes().is_empty());
    assert!(ov.take_dock_snaps().is_empty());
}

#[test]
fn grabbing_the_bar_still_moves_the_whole_dashboard() {
    let (mut vr, mut ov) = docked(false);
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
    let (mut vr, mut ov) = docked(false);
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

/// 見本のパネル（右手）を足す。
fn add_ghost(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay) -> PanelId {
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
    ghost
}

/// 見本の絵（今の解像度）が届いたことにする。
fn draw_ghost(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay, ghost: PanelId) {
    let (w, h) = ov.panel(ghost).resolution;
    ov.submit(&mut vr.backend, ghost, w, h, &vec![0; (w * h * 4) as usize]);
}

#[test]
fn the_ghost_frames_the_slot_without_moving_and_waits_for_its_picture() {
    let (mut vr, mut ov) = docked(true);
    let ghost = add_ghost(&mut vr, &mut ov);
    // 起動時の絵（前の大きさ）は届いていても、掴んだ画面に合わせた絵が届くまでは出さない（前の絵を一瞬見せない）。
    draw_ghost(&mut vr, &mut ov, ghost);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[ghost].visible);
    assert!(vr.backend.overlays[ghost].sort < vr.backend.overlays[1].sort);
    let slot = ov.panel(1).pose;
    controller(&mut vr, aim_at(slot.position), GRAB);
    ov.tick(&mut vr);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[ghost].visible, "絵が届くまで出さない");
    draw_ghost(&mut vr, &mut ov, ghost);
    // 届いた次からフェードインする。
    ov.tick(&mut vr);
    std::thread::sleep(std::time::Duration::from_millis(20));
    ov.tick(&mut vr);
    assert!(vr.backend.overlays[ghost].visible);
    // 地の縁より少し外まで、帯の分だけ上に伸び、Z ファイティングしないよう少し奥にある。
    let margin = PANEL_MARGIN * 0.3 / 300.0;
    let rect =
        Vec2::new(0.4, 0.3) - Vec2::splat(2.0 * margin) + Vec2::splat(2.0 * GHOST_PAD_PX / 1000.0);
    let band = DOCK_HINT_PX as f32 / 1000.0;
    let at = slot.position + Vec3::Y * band / 2.0 - Vec3::Z * GHOST_SINK;
    let res = (
        (rect.x * 1000.0).round() as u32,
        (rect.y * 1000.0).round() as u32 + DOCK_HINT_PX,
    );
    // 引いている間も、抜けて上下・左右に動かしている間も、見本は動かない（外れたナビゲーションバーの大きさに釣られない）。
    // 画面がはまる所から遠ざかるとフェードアウトし、近づくとフェードインする。濃さは距離で変えない（近いか遠いかの二択）。
    // 境目の辺り（0.5m）では、それまでの見え方のまま（行き来しない）。
    for (to, near) in [
        (Vec3::X * 0.1, true),
        (Vec3::X * 0.5, true),
        (Vec3::new(0.9, 0.6, 0.0), false),
        (Vec3::X * 0.5, false),
        (Vec3::new(0.2, 0.15, 0.0), true),
        (Vec3::new(0.15, -0.2, 0.1), true),
    ] {
        pull_to(&mut vr, &mut ov, slot.position + to);
        std::thread::sleep(std::time::Duration::from_millis(
            Motion::Enter.duration_ms() + 50,
        ));
        ov.tick(&mut vr);
        let o = &vr.backend.overlays[ghost];
        if !near {
            assert!(!o.visible, "{to}: 遠ざかると消える");
            continue;
        }
        assert!(
            o.visible && (o.alpha - 1.0).abs() < 1e-6,
            "{to}: {}",
            o.alpha
        );
        assert!((o.width_m - rect.x).abs() < 1e-4, "{}", o.width_m);
        assert!(
            o.pose.position.abs_diff_eq(at, 1e-4),
            "{to}: {}",
            o.pose.position
        );
        assert_eq!(o.pose.orientation, slot.orientation);
        assert_eq!(ov.panel(ghost).resolution, res);
    }
    assert!(ov.panel(1).parent.is_none());
    // 離すと薄れて消える。
    release(&mut vr, &mut ov, slot.position + Vec3::new(0.15, -0.2, 0.1));
    std::thread::sleep(std::time::Duration::from_millis(
        Motion::Exit.duration_ms() + 50,
    ));
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[ghost].visible);
}

/// 同じ画面を掴み直すと見本の大きさは変わらず、UI 層は同じ絵を送り直さない。それでも、届いている絵で見本を出す。
#[test]
fn regrabbing_shows_the_ghost_again_without_a_new_picture() {
    let (mut vr, mut ov) = docked(false);
    let ghost = add_ghost(&mut vr, &mut ov);
    let slot = ov.panel(1).pose.position;
    controller(&mut vr, aim_at(slot), GRAB);
    ov.tick(&mut vr);
    draw_ghost(&mut vr, &mut ov, ghost);
    let wait = |m: Motion| {
        std::thread::sleep(std::time::Duration::from_millis(m.duration_ms() + 50));
    };
    // `faded` が真なら、離して見本が消えきってから掴み直す。偽なら、消えかけている間に素早く掴み直す。
    for faded in [false, true, false] {
        ov.tick(&mut vr);
        wait(Motion::Enter);
        ov.tick(&mut vr);
        let o = &vr.backend.overlays[ghost];
        assert!(
            o.visible && (o.alpha - 1.0).abs() < 1e-6,
            "{faded}: {}",
            o.alpha
        );
        controller(&mut vr, aim_at(slot), &[]);
        ov.tick(&mut vr);
        if faded {
            wait(Motion::Exit);
            ov.tick(&mut vr);
            assert!(!vr.backend.overlays[ghost].visible);
        }
        controller(&mut vr, aim_at(slot), GRAB);
        ov.tick(&mut vr);
        assert_eq!(ov.dock_hint(Hand::Right), Some(DockHint::Pull));
    }
}
