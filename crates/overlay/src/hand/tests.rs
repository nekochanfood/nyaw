use super::*;
use crate::test_util::*;

#[test]
fn laser_click_on_panel() {
    let (mut vr, mut ov) = setup();
    let center = Vec2::new(50.0, 50.0);
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [
            PointerEvent::Move { pos: center },
            PointerEvent::Down { pos: center }
        ]
    );
    controller(&mut vr, Pose::IDENTITY, &[]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [
            PointerEvent::Move { pos: center },
            PointerEvent::Up { pos: center }
        ]
    );
}

#[test]
fn small_shake_while_pressed_keeps_the_press_point() {
    let (mut vr, mut ov) = setup();
    let center = Vec2::new(50.0, 50.0);
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    // 1cm（約 0.6°）ずれて離しても、押した所で離したことになる。
    let shaken = at(0.01, 0.0, 0.0);
    controller(&mut vr, shaken, &[("trigger/value", 1.0)]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move { pos: center }]
    );
    controller(&mut vr, shaken, &[]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [
            PointerEvent::Move { pos: center },
            PointerEvent::Up { pos: center }
        ]
    );
    // 離した後は手ブレのままの位置を指す。
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move {
            pos: Vec2::new(51.0, 50.0)
        }]
    );
    // 大きく動かすと、押したままでも追う。
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(0.1, 0.0, 0.0), &[("trigger/value", 1.0)]);
    assert!((move_x(&ov.tick(&mut vr)) - 60.0).abs() < 1e-3);
}

#[test]
fn panel_is_not_hit_from_behind() {
    let (mut vr, mut ov) = setup();
    // パネル（z = -1、表は +Z）の裏側（z = -2）から +Z へ向けて指しても、当たらない。
    let behind = Pose::new(
        Vec3::new(0.0, 0.0, -2.0),
        Quat::from_rotation_y(std::f32::consts::PI),
    );
    controller(&mut vr, behind, &[("trigger/value", 1.0)]);
    assert_eq!(kinds(&ov.tick(&mut vr)), []);
}

#[test]
fn hit_maps_to_top_left_pixels_and_leave() {
    let (mut vr, mut ov) = setup();
    controller(
        &mut vr,
        Pose::new(Vec3::new(-0.25, 0.25, 0.0), Quat::IDENTITY),
        &[],
    );
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move {
            pos: Vec2::new(25.0, 25.0)
        }]
    );
    controller(
        &mut vr,
        Pose::new(Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY),
        &[],
    );
    assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
}

#[test]
fn emulated_pointer_is_not_smoothed() {
    let (mut vr, mut ov) = setup();
    vr.backend.frame.emulated = true;
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    // 右へ向けると、手ブレ抑えなしでその場でヒット点が動く。
    let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-0.2));
    controller(&mut vr, aim, &[]);
    let PointerEvent::Move { pos } = ov.tick(&mut vr)[0].event else {
        panic!()
    };
    assert_eq!(Some(pos), hit_test(ov.panel(0), &aim).map(|h| h.0));
}

#[test]
fn pressed_pointer_keeps_its_panel_past_the_edge() {
    let (mut vr, mut ov) = setup();
    ov.set_capture(true);
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    // 押したまま右へはみ出しても、Leave にならずパネルの外の座標で Move が届く。
    let outside = Pose::new(Vec3::new(0.8, 0.0, 0.0), Quat::IDENTITY);
    controller(&mut vr, outside, &[("trigger/value", 1.0)]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move {
            pos: Vec2::new(130.0, 50.0)
        }]
    );
    // 離すと外で Up、次の tick で離れる。
    controller(&mut vr, outside, &[]);
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [
            PointerEvent::Move {
                pos: Vec2::new(130.0, 50.0)
            },
            PointerEvent::Up {
                pos: Vec2::new(130.0, 50.0)
            }
        ]
    );
    assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
}

#[test]
fn released_capture_lets_the_pointer_move_to_another_panel() {
    let (mut vr, mut ov) = setup();
    ov.set_capture(true);
    let right = Pose::new(Vec3::new(1.2, 0.0, -1.0), Quat::IDENTITY);
    ov.add_panel(
        &mut vr.backend,
        "right",
        right,
        Vec2::new(1.0, 1.0),
        (100, 100),
    )
    .unwrap();
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    // ドラッグに切り替わった（キャプチャを外した）ので、押したまま隣のパネルへ移れる。
    ov.release_capture(Hand::Right);
    let over_right = Pose::new(Vec3::new(1.2, 0.0, 0.0), Quat::IDENTITY);
    controller(&mut vr, over_right, &[("trigger/value", 1.0)]);
    let ev = ov.tick(&mut vr);
    assert!(
        ev.iter()
            .any(|e| e.panel == 1 && matches!(e.event, PointerEvent::Move { .. })),
        "{ev:?}"
    );
}

#[test]
fn lost_controller_leaves_the_panel_once() {
    let (mut vr, mut ov) = setup();
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    vr.backend.frame.controllers.clear();
    assert_eq!(kinds(&ov.tick(&mut vr)), [PointerEvent::Leave]);
    assert!(ov.tick(&mut vr).is_empty());
}

#[test]
fn grab_moves_panel_and_syncs_to_vr() {
    let (mut vr, mut ov) = setup();
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    let moved = Pose::new(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY);
    controller(&mut vr, moved, &[("squeeze/value", 1.0)]);
    assert!(ov.tick(&mut vr).is_empty());
    settle(&mut vr, &mut ov);
    let expected = Vec3::new(1.0, 0.0, -1.0);
    assert!(ov.panel(0).pose.position.abs_diff_eq(expected, 1e-5));
    assert!(
        vr.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq(expected, 1e-5)
    );
}

/// `pointing_hands` はパネルそのものを指している手だけ（ナビゲーションバーを指していても持ち主は含めない。外れれば空）。
#[test]
fn pointing_hands_are_those_aiming_at_the_panel_itself() {
    let (mut vr, mut ov) = setup();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            Pose::IDENTITY,
            Vec2::new(0.2, 0.05),
            (200, 50),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    // バーが持ち主の下に付くまで 1 回回す。
    ov.tick(&mut vr);
    assert!(ov.pointing_hands(0).is_empty());
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert_eq!(ov.pointing_hands(0), vec![Hand::Right]);
    assert!(ov.pointing_hands(bar).is_empty(), "バーは指していない");
    controller(
        &mut vr,
        Pose::new(Vec3::ZERO, Quat::from_rotation_y(1.5)),
        &[],
    );
    ov.tick(&mut vr);
    assert!(ov.pointing_hands(0).is_empty(), "外れれば空");
}

/// 何も無い所を押すと数え、パネルを押したときは数えない。
#[test]
fn presses_on_nothing_are_counted() {
    let (mut vr, mut ov) = setup();
    // パネル（正面 1m）を押す。
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert_eq!(ov.outside_presses(), 0);
    // 横の何も無い所を押す。
    controller(&mut vr, at(5.0, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    controller(&mut vr, at(5.0, 0.0, 0.0), &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    assert_eq!(ov.outside_presses(), 1);
}

/// 暗転している間（ダッシュボードが出ている間）はゲームへの入力を止め、隠す・閉じると戻す。
#[test]
fn app_input_is_blocked_while_the_veil_is_up() {
    let (mut vr, mut ov) = setup();
    ov.set_veil(0, true);
    ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
    ov.tick(&mut vr);
    assert!(vr.backend.app_input_blocked);
    ov.set_hidden(&mut vr.backend, 0, true);
    ov.tick(&mut vr);
    assert!(!vr.backend.app_input_blocked);
    ov.set_hidden(&mut vr.backend, 0, false);
    ov.tick(&mut vr);
    assert!(vr.backend.app_input_blocked);
    ov.close(&mut vr.backend, 0);
    ov.tick(&mut vr);
    assert!(!vr.backend.app_input_blocked);
}

/// 入力の遮断を切ると、暗転していてもゲームへの入力を通す。出ている最中に切り替えても次の tick で効く。
#[test]
fn app_input_can_pass_through_the_veil() {
    let (mut vr, mut ov) = setup();
    ov.set_veil(0, true);
    ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
    ov.tick(&mut vr);
    assert!(vr.backend.app_input_blocked);
    ov.set_block_input(false);
    ov.tick(&mut vr);
    assert!(!vr.backend.app_input_blocked);
    ov.set_block_input(true);
    ov.tick(&mut vr);
    assert!(vr.backend.app_input_blocked);
}

#[test]
fn reports_trackers_menu_and_grab() {
    let (mut vr, mut ov) = setup();
    vr.backend.frame.trackers = vec![("LHR-1".into(), Pose::IDENTITY)];
    controller(
        &mut vr,
        Pose::IDENTITY,
        &[("b/click", 1.0), ("squeeze/value", 1.0)],
    );
    ov.tick(&mut vr);
    assert_eq!(ov.trackers(), ["LHR-1"]);
    assert!(!ov.quit_requested());
    vr.backend.frame.quit = true;
    ov.tick(&mut vr);
    assert!(ov.quit_requested());
    vr.backend.frame.quit = false;
    assert!(ov.menu_held() && ov.grabbed(0));
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert!(!ov.menu_held() && !ov.grabbed(0));
}

#[test]
fn pulling_the_trigger_steadies_and_fast_swings_follow() {
    // 同じだけ向きを変えたとき、1 tick で進んだ量（ピクセル）。
    let step = |turn: f32, trigger: f32| {
        let (mut vr, mut ov) = setup();
        // 仕組みを見るため、既定値によらない強さ・緩める角度（約 1.1°）にする。
        ov.set_pointer_smoothing(0.9);
        ov.set_pointer_release(0.02f32.to_degrees());
        controller(&mut vr, Pose::IDENTITY, &[("trigger/value", trigger)]);
        ov.tick(&mut vr);
        let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-turn));
        controller(&mut vr, aim, &[("trigger/value", trigger)]);
        (move_x(&ov.tick(&mut vr)) - 50.0) / (hit_test(ov.panel(0), &aim).unwrap().0.x - 50.0)
    };
    // 押下前にトリガーを引いている間は、手ブレをほとんど拾わない。
    let (free, pulling) = (step(0.01, 0.0), step(0.01, 0.55));
    assert!(pulling < free / 2.0, "{pulling} {free}");
    // 速く振るほど遅れない。
    let fast = step(0.3, 0.0);
    assert!(fast > 0.8 && fast > free, "{fast} {free}");
}

#[test]
fn smoothing_steadies_hits_and_pointer_together() {
    let (mut vr, mut ov, laser, cursor) = with_pointers();
    // 仕組みを見るため、既定値によらない強さ・緩める角度・線の長さにする。
    ov.set_pointer_smoothing(0.9);
    ov.set_pointer_release(0.02f32.to_degrees());
    ov.set_laser_length(LASER_MAX);
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    // 右へ向けると、ヒット（クリック位置）もポインタも途中から追う。
    let turned = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-0.3));
    let target = hit_test(ov.panel(0), &turned).unwrap().0.x;
    controller(&mut vr, turned, &[]);
    let x = move_x(&ov.tick(&mut vr));
    assert!(50.0 < x && x < target, "{x} {target}");
    // カーソルはヒット点（パネルの面の上）、レーザーの先端にある。
    let c = vr.backend.overlays[cursor].pose.position;
    let hit = ov.panel(0).pixel_to_world(Vec2::new(x, 50.0));
    assert!(
        c.abs_diff_eq(hit + Vec3::Z * CURSOR_LIFT, 1e-4),
        "{c} {hit}"
    );
    let tip = vr.backend.overlays[laser].pose.position * 2.0;
    assert!(
        tip.abs_diff_eq(hit - hit.normalize() * LASER_GAP, 1e-4),
        "{tip}"
    );
    for _ in 0..200 {
        ov.tick(&mut vr);
    }
    assert!((move_x(&ov.tick(&mut vr)) - target).abs() < 0.01);
    // 抑えを切れば即座に当たる。
    ov.set_pointer_smoothing(0.0);
    controller(&mut vr, Pose::IDENTITY, &[]);
    assert_eq!(move_x(&ov.tick(&mut vr)), 50.0);
}

#[test]
fn grabbed_panel_faces_head_and_stays_level() {
    let (mut vr, mut ov) = setup();
    // 手ブレ抑えが効くと掴んだパネルは抑えたレーザーに付いて遅れるので、ここでは向きだけを確かめる。
    ov.set_pointer_smoothing(0.0);
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    // 頭を傾けて横にずらし、手首もひねる。パネルは頭の方を向き、傾かない。
    vr.backend.frame.hmd = Pose::new(Vec3::new(0.5, 0.3, 0.2), Quat::from_rotation_z(0.3));
    let aim = Pose::new(
        Vec3::ZERO,
        Quat::from_rotation_y(0.5) * Quat::from_rotation_z(1.0),
    );
    controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    let p = ov.panel(0).pose;
    assert!(
        p.position
            .abs_diff_eq(aim.transform_point(Vec3::new(0.0, 0.0, -1.0)), 1e-5),
        "{} {}",
        p.position,
        aim.transform_point(Vec3::new(0.0, 0.0, -1.0))
    );
    let normal = p.orientation * Vec3::Z;
    let to_head = (vr.backend.frame.hmd.position - p.position).normalize();
    assert!(normal.abs_diff_eq(to_head, 1e-5), "{normal}");
    assert!((p.orientation * Vec3::X).y.abs() < 1e-5, "horizontal");
}

#[test]
fn grabbed_panel_follows_the_head_tilt_while_lying() {
    let (mut vr, mut ov) = setup();
    ov.set_pointer_smoothing(0.0);
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_y(0.5));
    // 横向きに寝転がり、首をさらに傾けていく。パネルは頭の方を向き、上は頭の上に付いてくる。
    for roll in [1.3_f32, 1.57, 1.9] {
        let head = Quat::from_rotation_z(roll);
        vr.backend.frame.hmd = Pose::new(Vec3::new(0.5, 0.3, 0.2), head);
        controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        let p = ov.panel(0).pose;
        let normal = p.orientation * Vec3::Z;
        let to_head = (vr.backend.frame.hmd.position - p.position).normalize();
        assert!(normal.abs_diff_eq(to_head, 1e-5), "{normal}");
        let head_up = head * Vec3::Y;
        assert!(
            (p.orientation * Vec3::X).dot(head_up).abs() < 1e-5,
            "{roll}"
        );
        assert!((p.orientation * Vec3::Y).dot(head_up) > 0.5, "{roll}");
    }
    // 起き上がると、これまでどおり水平に戻る。
    vr.backend.frame.hmd = Pose::new(Vec3::new(0.5, 0.3, 0.2), Quat::from_rotation_z(0.3));
    controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    assert!((ov.panel(0).pose.orientation * Vec3::X).y.abs() < 1e-5);
}

#[test]
fn grab_glides_toward_hand() {
    let (mut vr, mut ov) = setup();
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    // 1 tick では途中まで。
    let x = ov.panel(0).pose.position.x;
    assert!(0.0 < x && x < 1.0, "{x}");
}

#[test]
fn trigger_and_move_scales_grabbed_panel_in_place() {
    let (mut vr, mut ov) = setup();
    ov.set_pointer_smoothing(0.0);
    let both = [("squeeze/value", 1.0), ("trigger/value", 1.0)];
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    let (w0, pos) = (ov.panel(0).size.x, ov.panel(0).pose.position);
    controller(&mut vr, Pose::IDENTITY, &both);
    ov.tick(&mut vr);
    // 前へ出すと大きく、その場に留まる。
    controller(&mut vr, at(0.0, 0.0, -0.2), &both);
    settle(&mut vr, &mut ov);
    let w1 = ov.panel(0).size.x;
    assert!((w1 / w0 - (0.2 * SCALE_RATE).exp()).abs() < 1e-3, "{w1}");
    assert!(ov.panel(0).pose.position.abs_diff_eq(pos, 1e-3));
    // 引くと縮む。
    controller(&mut vr, Pose::IDENTITY, &both);
    settle(&mut vr, &mut ov);
    assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
    // トリガーなしで動かしても大きさは変わらない。
    controller(&mut vr, at(0.0, 0.0, -0.2), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
}

#[test]
fn resize_input_scales_only_the_grabbed_panel() {
    let (mut vr, mut ov) = setup();
    const RESIZE: &str = subsystems::vr::RESIZE_INPUT;
    let resize = |r| [("squeeze/value", 1.0), (RESIZE, r)];
    controller(&mut vr, Pose::IDENTITY, &resize(0.0));
    settle(&mut vr, &mut ov);
    let (w0, h0, pos) = (
        ov.panel(0).size.x,
        ov.panel(0).size.y,
        ov.panel(0).pose.position,
    );
    // 押している間、tick ごとに大きくなる。縦横比は保ち、その場に留まる。
    controller(&mut vr, Pose::IDENTITY, &resize(1.0));
    for _ in 0..10 {
        ov.tick(&mut vr);
    }
    let w1 = ov.panel(0).size.x;
    assert!((w1 / w0 - (10.0 * RESIZE_STEP).exp()).abs() < 1e-3, "{w1}");
    assert!((ov.panel(0).size.y / h0 - w1 / w0).abs() < 1e-4);
    assert!(ov.panel(0).pose.position.abs_diff_eq(pos, 1e-3));
    // 逆向きで同じだけ縮むと元に戻る。
    controller(&mut vr, Pose::IDENTITY, &resize(-1.0));
    for _ in 0..10 {
        ov.tick(&mut vr);
    }
    assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
    // 掴んでいなければ変わらない。
    controller(&mut vr, Pose::IDENTITY, &[]);
    settle(&mut vr, &mut ov);
    controller(&mut vr, Pose::IDENTITY, &[(RESIZE, 1.0)]);
    settle(&mut vr, &mut ov);
    assert!((ov.panel(0).size.x - w0).abs() < 1e-3);
}

#[test]
fn stick_pushes_and_pulls_grabbed_panel() {
    let (mut vr, mut ov) = setup();
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    let dist = |ov: &Overlay| -ov.panel(0).pose.position.z;
    let push = |y| [("squeeze/value", 1.0), ("thumbstick/y", y)];
    controller(&mut vr, Pose::IDENTITY, &push(1.0));
    settle(&mut vr, &mut ov);
    controller(&mut vr, Pose::IDENTITY, &push(0.0));
    settle(&mut vr, &mut ov);
    let far = dist(&ov);
    assert!(far > 1.5, "{far}");
    // 遊びの中では動かない。
    controller(&mut vr, Pose::IDENTITY, &push(0.1));
    settle(&mut vr, &mut ov);
    assert!((dist(&ov) - far).abs() < 1e-4);
    // 引き寄せても手より手前には来ない。
    controller(&mut vr, Pose::IDENTITY, &push(-1.0));
    for _ in 0..1000 {
        ov.tick(&mut vr);
    }
    assert!((dist(&ov) - GRAB_MIN).abs() < 1e-4, "{}", dist(&ov));
}

/// 奥行きの変化は距離によらず一定の速さ（近くでも遅くならない）。
#[test]
fn stick_push_is_linear_in_distance() {
    let (mut vr, mut ov) = setup();
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    let dist = |ov: &Overlay| -ov.panel(0).pose.position.z;
    let pull = |vr: &mut Subsystems<MockBackend>, ov: &mut Overlay| {
        controller(
            vr,
            Pose::IDENTITY,
            &[("squeeze/value", 1.0), ("thumbstick/y", -1.0)],
        );
        for _ in 0..10 {
            ov.tick(vr);
        }
        controller(vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        settle(vr, ov);
    };
    let d0 = dist(&ov);
    pull(&mut vr, &mut ov);
    let d1 = dist(&ov);
    pull(&mut vr, &mut ov);
    let d2 = dist(&ov);
    assert!(((d0 - d1) - (d1 - d2)).abs() < 0.02, "{d0} {d1} {d2}");
}

#[test]
fn stick_moves_grabbed_point_along_the_laser() {
    let (mut vr, mut ov) = setup();
    ov.set_pointer_smoothing(0.0);
    // パネルの右寄り（中心から 0.3m）を斜めに掴み、押し出す。
    let aim = Pose::new(Vec3::ZERO, Quat::from_rotation_y(-0.3f32.atan()));
    controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    let before = ov.panel(0).pose.position;
    controller(
        &mut vr,
        aim,
        &[("squeeze/value", 1.0), ("thumbstick/y", 1.0)],
    );
    for _ in 0..20 {
        ov.tick(&mut vr);
    }
    controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    // 中心（= 掴んだ点）はレーザーと平行に、手から遠ざかる向きへ動く。
    let moved = ov.panel(0).pose.position - before;
    assert!(moved.length() > 0.3);
    assert!(
        moved.normalize().abs_diff_eq(aim.forward(), 1e-4),
        "{moved}"
    );
}

/// 基準（`grab_anchor`）を持つパネルは、掴むと基準を手で動かして頭へ向け、パネルはそこに付いてくる。
#[test]
fn grabbing_a_panel_with_an_anchor_turns_the_anchor_to_the_head() {
    let (mut vr, mut ov) = setup();
    // 画面が立つ所: パネルの 0.6m 上で、パネルから見て 0.5 rad 起こした向き。
    let anchor = Pose::new(Vec3::Y * 0.6, Quat::from_rotation_x(0.5));
    ov.set_grab_anchor(0, Some(anchor));
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    assert!(ov.grabbed(0));
    controller(&mut vr, at(0.5, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    settle(&mut vr, &mut ov);
    let reference = ov.panel(0).pose * anchor;
    // 基準の面は頭（原点）を向き、パネルは基準から決まる所にある。
    let to_eye = (Vec3::ZERO - reference.position).normalize();
    assert!((reference.orientation * Vec3::Z).abs_diff_eq(to_eye, 1e-3));
    assert!(reference.position.x > 0.4);
}

/// 基準を持つパネルを掴んで大きさを変えると、基準も大きさに合わせて動く（キーボードのホームポジションに付いたまま）。
#[test]
fn resizing_moves_the_grab_anchor_with_the_panel() {
    const RESIZE: &str = subsystems::vr::RESIZE_INPUT;
    let (mut vr, mut ov) = setup();
    ov.set_grab_anchor(0, Some(Pose::new(Vec3::X * 0.3, Quat::IDENTITY)));
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    assert!(ov.grabbed(0));
    let width = ov.panel(0).size.x;
    controller(
        &mut vr,
        Pose::IDENTITY,
        &[("squeeze/value", 1.0), (RESIZE, 1.0)],
    );
    ov.tick(&mut vr);
    let scale = ov.panel(0).size.x / width;
    assert!(scale > 1.0);
    let anchor = ov.panel(0).grab_anchor.unwrap();
    assert!((anchor.position.x - 0.3 * scale).abs() < 1e-5, "{anchor:?}");
}
