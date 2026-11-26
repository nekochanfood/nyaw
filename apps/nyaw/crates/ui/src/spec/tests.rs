use super::*;
use crate::test_util::*;

#[gpui::test]
fn source_nav_bar_has_minimize_beside_close(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(2)
            .or_default()
            .status
            .pointed = true;
        NavBar {
            id: 2,
            owner: 1,
            title: None,
            os_keyboard: false,
            dockable: false,
            minimize: None,
            source_minimize: Some(std::rc::Rc::new(|id, cx| {
                command(id, PanelCommand::Hide(true), cx)
            })),
            open: design::animation::Transition::rest(1.0),
            held: false,
        }
    });
    cx.run_until_parked();
    let minimize = cx.debug_bounds("nav-minimize-source").unwrap();
    let close = cx.debug_bounds("nav-close").unwrap();
    assert!(minimize.right() <= close.left());
    cx.simulate_click(minimize.center(), gpui::Modifiers::none());
    cx.simulate_click(close.center(), gpui::Modifiers::none());
    let commands = &cx.update(|_, cx| cx.global::<PanelControl>().panels[&1].commands.clone());
    assert_eq!(commands, &[PanelCommand::Hide(true), PanelCommand::Close]);
}

/// タイピング位置に配置するパネルは、手のやや上方に配置し、面は視線を重視しつつ手の方向にも若干傾ける。
#[test]
fn typing_panel_sits_just_above_the_hands_and_faces_mostly_the_eyes() {
    let mut kernel =
        Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    kernel.tick();
    let eye = kernel.hmd().position;
    let hand = |hand, x| kernel::vr::RawController {
        hand,
        profile: "valve/index_controller".into(),
        aim: Pose::new(eye + Vec3::new(x, -0.45, -0.3), Quat::IDENTITY),
        inputs: Vec::new(),
    };
    kernel.subsystems.backend.frame.controllers =
        vec![hand(Hand::Left, -0.2), hand(Hand::Right, 0.2)];
    kernel.tick();
    let spec = PanelSpec::root(Pose::IDENTITY, |_, _, _| gpui::Empty)
        .sized(Vec2::new(1.0, 0.2), (100, 20))
        .typing(0.0);
    let pose = spec.pose_for(&kernel, 0);
    let height = pose.position.y - eye.y;
    assert!(
        (height - (-0.45 + TYPING_ABOVE_HANDS)).abs() < 1e-4,
        "{height}"
    );
    let hands = eye + Vec3::new(0.0, -0.45, -0.3);
    let normal = pose.orientation * Vec3::Z;
    let aim = eye.lerp(hands, TYPING_TOWARD_HANDS);
    assert!(normal.dot((aim - pose.position).normalize()) > 1.0 - 1e-4);
    // 視線方向より手の方向（下方）に寄る。
    assert!(normal.dot((eye - pose.position).normalize()) < 1.0 - 1e-4);
    // 手を完全に下ろしていても、視認しやすい高さより下には配置しない。
    let mut low = hand(Hand::Right, 0.2);
    low.aim.position.y = eye.y - 1.2;
    kernel.subsystems.backend.frame.controllers = vec![low];
    kernel.tick();
    let height = spec.pose_for(&kernel, 0).position.y - eye.y;
    assert!((height - TYPING_LOWEST).abs() < 1e-4, "{height}");
    // 手を上げていても、上端は視線より下方に保つ。
    let mut high = hand(Hand::Right, 0.2);
    high.aim.position.y = eye.y + 0.3;
    kernel.subsystems.backend.frame.controllers = vec![high];
    kernel.tick();
    let height = spec.pose_for(&kernel, 0).position.y - eye.y;
    assert!(
        (height - (-0.1 - TYPING_CLEARANCE)).abs() < 1e-4,
        "{height}"
    );
}

/// 呼び出し元より手前に配置するため、視点から見て呼び出し元の下端（ナビゲーションバー込み）より下方に見えるよう下げる。
#[test]
fn typing_panel_shows_below_the_caller_without_covering_it() {
    let mut kernel =
        Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    kernel.tick();
    let eye = kernel.hmd().position;
    let caller = kernel
        .add_panel(
            "caller",
            Pose::new(eye + Vec3::new(0.0, -0.1, -1.2), Quat::IDENTITY),
            Vec2::new(1.2, 0.8),
            (1200, 800),
        )
        .unwrap();
    let spec = PanelSpec::spawn(caller, |_, _, _| gpui::Empty)
        .sized(Vec2::new(1.0, 0.33), (1000, 330))
        .typing(0.0);
    let pose = spec.pose_for(&kernel, caller + 1);
    let elevation = |p: Vec3| (p.y - eye.y).atan2(eye.z - p.z);
    let top = elevation(pose.transform_point(Vec3::Y * 0.165));
    let bottom = elevation(eye + Vec3::new(0.0, -0.5, -1.2));
    assert!(top < bottom - TYPING_PANEL_GAP * 0.5, "{top} {bottom}");
    // 必要な分だけ下げる（過度に離さない）。
    assert!(top > bottom - TYPING_PANEL_GAP * 2.0, "{top} {bottom}");
}

/// 左右の向きと位置は呼び出し元に合わせ、頭を左右に向けても変化しない（頭の向きが影響するのは上下方向のみ）。
#[test]
fn typing_panel_follows_the_callers_heading_not_the_heads() {
    let mut kernel =
        Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    kernel.tick();
    let eye = kernel.hmd().position;
    let yaw = Quat::from_rotation_y(0.3);
    let caller = kernel
        .add_panel(
            "caller",
            Pose::new(eye + yaw * Vec3::new(0.0, 0.3, -1.2), yaw),
            Vec2::new(1.0, 0.6),
            (1000, 600),
        )
        .unwrap();
    let spec = PanelSpec::spawn(caller, |_, _, _| gpui::Empty)
        .sized(Vec2::new(1.0, 0.2), (100, 20))
        .typing(0.1);
    let mut poses = Vec::new();
    for head_yaw in [-0.5, 0.0, 0.6] {
        kernel.subsystems.backend.frame.hmd = Pose::new(eye, Quat::from_rotation_y(head_yaw));
        kernel.tick();
        poses.push(spec.pose_for(&kernel, caller + 1));
    }
    for pose in &poses {
        assert!(
            pose.position.abs_diff_eq(poses[0].position, 1e-4),
            "{pose:?}"
        );
        assert!(pose.orientation.abs_diff_eq(poses[0].orientation, 1e-4));
        // 面の水平方向の向きは呼び出し元と同一。
        let normal = pose.orientation * Vec3::Z;
        let flat = Vec3::new(normal.x, 0.0, normal.z).normalize();
        assert!(flat.abs_diff_eq(yaw * Vec3::Z, 1e-4), "{flat}");
        // ホームポジション（中心から 0.1m）が呼び出し元の中心の左右位置に一致する。
        let home = yaw.inverse() * (pose.transform_point(Vec3::X * 0.1) - eye);
        assert!(home.x.abs() < 1e-4, "{home}");
    }
}

/// 仰臥時は手の高さによらず、視界内の正面下方（正面パネルと重ならない位置）に配置する。
#[test]
fn typing_panel_sits_low_in_view_while_lying() {
    let mut kernel =
        Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    // 仰臥姿勢（真上を見る）。
    let head = Pose::new(
        Vec3::new(0.0, 0.3, 0.0),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    kernel.subsystems.backend.frame.hmd = head;
    kernel.subsystems.backend.frame.controllers = vec![kernel::vr::RawController {
        hand: Hand::Right,
        profile: "valve/index_controller".into(),
        aim: Pose::new(Vec3::new(0.2, 0.6, 0.0), Quat::IDENTITY),
        inputs: Vec::new(),
    }];
    kernel.tick();
    let spec = PanelSpec::root(Pose::IDENTITY, |_, _, _| gpui::Empty)
        .sized(Vec2::new(1.0, 0.2), (100, 20))
        .typing(0.0);
    let pose = spec.pose_for(&kernel, 0);
    let local = kernel.hmd().inverse().transform_point(pose.position);
    let expected = Vec3::new(0.0, -0.1 - TYPING_LYING_CLEARANCE, TYPING_AT.z);
    assert!(local.abs_diff_eq(expected, 1e-3), "{local}");
    // 面は視点に向く。
    let normal = pose.orientation * Vec3::Z;
    let toward = (kernel.hmd().position - pose.position).normalize();
    assert!(normal.dot(toward) > 1.0 - 1e-4);
}

#[gpui::test]
fn spec_attaches_locks_and_sizes_panel(cx: &mut TestAppContext) {
    let mut kernel = Kernel::boot(Some("novr"), Default::default(), false).unwrap();
    let hand = Device::Controller(Hand::Left);
    let spec = PanelSpec::root(Pose::IDENTITY, |_, _, _| Probe::default())
        .sized(Vec2::new(0.1, 0.05), (10, 5))
        .attached(hand.clone(), Pose::IDENTITY)
        .locked()
        .without_animation();
    let (panel, _) = cx
        .update(|cx| open_panel(&mut kernel, spec, 0, cx))
        .unwrap();
    let p = kernel.panel(panel);
    assert_eq!(p.attached, Some((hand, Pose::IDENTITY)));
    assert!(p.locked);
    assert!(!p.animated);
    assert_eq!((p.size, p.resolution), (Vec2::new(0.1, 0.05), (10, 5)));
}

/// 視界前方に配置するパネルは頭の水平方向に置き、うつむいていても垂直に配置する。
#[test]
fn in_view_places_level_in_front_of_head() {
    // +X を向き、30° うつむいた頭。
    let head = Pose::new(
        Vec3::new(1.0, 1.6, 0.0),
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(-0.5),
    );
    let p = head.leveled() * Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
    assert!(p.position.abs_diff_eq(Vec3::new(2.0, 1.6, 0.0), 1e-5));
    assert!((p.orientation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
}

#[gpui::test]
fn panel_window_is_hidden_and_matches_panel_resolution(cx: &mut TestAppContext) {
    let options = panel_window_options(PANEL_PX);
    assert!(!options.show && !options.focus);
    assert!(matches!(
        options.window_background,
        gpui::WindowBackgroundAppearance::Transparent
    ));
    let window = cx.update(|cx| {
        open_panel_window(cx, PANEL_PX, |_, cx| cx.new(|_| Probe::default())).unwrap()
    });
    let vp = cx
        .update(|cx| cx.update_window(window.into(), |_, w, _| w.viewport_size()))
        .unwrap();
    assert_eq!(vp, gpui::size(px(PANEL_PX.0 as f32), px(PANEL_PX.1 as f32)));
}

/// ボタンはすべて右寄せで、右から閉じる・配信元を最小化・OS キーボード・「…」の順に詰めて配置する。手に持つ・外す・戻すは
/// バーに表示せず、「…」がポップオーバーを展開する。
#[gpui::test]
fn every_button_sits_on_the_right_ending_with_more(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| {
        let ctl = cx.default_global::<PanelControl>();
        ctl.dock = Some((5, 0.0));
        ctl.panels.entry(2).or_default().status.pointed = true;
        NavBar {
            id: 2,
            owner: 1,
            title: None,
            os_keyboard: true,
            dockable: true,
            minimize: None,
            source_minimize: Some(std::rc::Rc::new(|_, _| {})),
            open: design::animation::Transition::rest(1.0),
            held: false,
        }
    });
    cx.run_until_parked();
    for id in ["nav-hand", "nav-dock", "nav-undock"] {
        assert!(cx.debug_bounds(id).is_none(), "{id}");
    }
    let row = [
        "nav-more",
        "nav-keyboard",
        "nav-minimize-source",
        "nav-close",
    ]
    .map(|s| cx.debug_bounds(s).unwrap());
    let width = cx.update(|w, _| w.viewport_size().width);
    assert!(row[0].left() > width / 2.0, "{:?} {width:?}", row[0]);
    for pair in row.windows(2) {
        let gap = pair[1].left() - pair[0].right();
        assert!(
            gap >= px(0.0) && gap <= px(design::spacing::SM + 1.0),
            "{gap:?}"
        );
    }
    cx.simulate_click(row[0].center(), gpui::Modifiers::none());
    assert_eq!(
        cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
        1
    );
}
