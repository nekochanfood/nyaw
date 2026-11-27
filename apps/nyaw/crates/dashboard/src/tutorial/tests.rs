use super::*;

/// 手順は開始画面に始まり完了で終わり、`next` は [`STEPS`] の順に進み、`back` は逆に戻る（両端はそのまま）。
#[test]
fn steps_go_from_the_intro_to_done_and_back() {
    assert_eq!(STEPS.first(), Some(&Step::Intro));
    assert_eq!(STEPS.last(), Some(&Step::Done));
    for pair in STEPS.windows(2) {
        assert_eq!(pair[0].next(), pair[1]);
        assert_eq!(pair[1].back(), pair[0]);
    }
    assert_eq!(Step::Done.next(), Step::Done);
    assert_eq!(Step::Intro.back(), Step::Intro);
}

/// 開始画面ははめ、ほかは説明する所の近くに浮かべる。バーの説明は画面がはまっていなければバーの上、はまっていれば画面の右。
#[test]
fn the_coach_goes_near_what_each_step_explains() {
    assert_eq!(Step::Intro.spot(true), Spot::Docked);
    assert_eq!(Step::QuickSettings.spot(true), Spot::Bar);
    assert_eq!(Step::Launcher.spot(false), Spot::Beside);
    assert_eq!(Step::Grab.spot(false), Spot::Beside);
    assert_eq!(Step::NavBar.spot(false), Spot::Below);
    assert_eq!(
        Step::NavBar.spot(true),
        Spot::Docked,
        "画面を閉じたら、はめる"
    );
    assert_eq!(Step::Minimize.spot(false), Spot::Below);
}

/// ダッシュボードの距離・高さの設定（既定と両端）。
const SETTINGS: [(f32, f32); 3] = [
    (kernel::DASHBOARD_DISTANCE, 0.0),
    (kernel::DASHBOARD_DISTANCE_MIN, kernel::DASHBOARD_HEIGHT_MAX),
    (kernel::DASHBOARD_DISTANCE_MAX, kernel::DASHBOARD_HEIGHT_MIN),
];

/// どの置き場所でも頭からの距離が同じで、見かけの大きさが変わらない。立てたまま、水平には頭のほうへ向く。
#[test]
fn the_coach_keeps_its_distance_and_faces_the_head() {
    for (distance, height) in SETTINGS {
        let r = coach_pose(Spot::Docked, distance, height).position.length();
        for spot in [Spot::Bar, Spot::Beside, Spot::Below] {
            let coach = coach_pose(spot, distance, height);
            assert!((coach.position.length() - r).abs() < 1e-4, "{spot:?}");
            let facing = coach.orientation * Vec3::Z;
            assert!(facing.y.abs() < 1e-4, "{facing}");
            let head = Vec3::new(-coach.position.x, 0.0, -coach.position.z).normalize();
            assert!(
                facing.dot(head) > 0.999 || spot == Spot::Bar,
                "{spot:?} {facing}"
            );
        }
    }
}

/// 画面の右では、頭から見て、はめた画面の右端から少し空けて左端が来る（画面に重ならない）。下寄りは低いだけ。
#[test]
fn beside_the_screen_the_coach_stands_clear_of_it() {
    let yaw = |p: Vec3| p.x.atan2(-p.z);
    for (distance, height) in SETTINGS {
        let screen = yaw(Vec3::new(SIZE_M.x / 2.0, 0.0, -distance));
        for spot in [Spot::Beside, Spot::Below] {
            let coach = coach_pose(spot, distance, height);
            let left = coach.transform_point(Vec3::new(-COACH_M.x / 2.0, 0.0, 0.0));
            assert!((yaw(left) - screen - COACH_GAP).abs() < 1e-3, "{spot:?}");
        }
        let (beside, below) = (
            coach_pose(Spot::Beside, distance, height),
            coach_pose(Spot::Below, distance, height),
        );
        assert!((beside.position.y - (HEIGHT_ORIGIN + height)).abs() < 1e-4);
        assert!((beside.position.y - below.position.y - COACH_DROP).abs() < 1e-4);
    }
}

/// バーの上は、はめた所と同じ位置（外れて浮かぶだけ）で、頭から見てバーの上端より上に下端がある。
#[test]
fn above_the_bar_the_coach_clears_it_as_seen_from_the_head() {
    let margin = components::PANEL_MARGIN / 1000.0;
    for (distance, height) in SETTINGS {
        assert_eq!(
            coach_pose(Spot::Bar, distance, height),
            coach_pose(Spot::Docked, distance, height)
        );
        let bar = pose(distance, height);
        let top = bar.transform_point(Vec3::Y * (BAR_M.y / 2.0 - margin));
        let coach = coach_pose(Spot::Bar, distance, height);
        let coach_margin = components::PANEL_MARGIN * COACH_M.y / COACH_PX.1 as f32;
        let bottom = coach.transform_point(Vec3::NEG_Y * (COACH_M.y / 2.0 - coach_margin));
        let angle = |p: Vec3| p.y.atan2(-p.z);
        assert!(angle(bottom) > angle(top), "{bottom} {top}");
    }
}

/// はめた案内のパネルは、オーバーレイ層がバーの上のはまる所に置く姿勢と一致する（GPU の通し確認がこれでボタンを押す）。
#[test]
fn the_docked_coach_is_where_the_overlay_puts_it() {
    use kernel::overlay::{Overlay, Parent};
    use kernel::subsystems::Subsystems;
    use kernel::vr::mock::MockBackend;
    for (distance, height) in SETTINGS {
        let mut vr = Subsystems::with_backend(MockBackend::default());
        let mut ov = Overlay::new();
        let bar = ov
            .add_panel(
                &mut vr.backend,
                "bar",
                pose(distance, height),
                BAR_M,
                BAR_PX,
            )
            .unwrap();
        let coach = ov
            .add_panel(&mut vr.backend, "coach", SCREEN_POSE, COACH_M, COACH_PX)
            .unwrap();
        ov.set_animated(&mut vr.backend, coach, false);
        ov.set_parent(
            &mut vr.backend,
            coach,
            Some(Parent {
                panel: bar,
                carry: true,
                docked: true,
                slot: Some(BAR_TILT),
                ..Default::default()
            }),
        );
        for _ in 0..3 {
            ov.tick(&mut vr);
        }
        let want = coach_pose(Spot::Docked, distance, height);
        let got = ov.panel(coach).pose;
        assert!(
            got.position.abs_diff_eq(want.position, 1e-3),
            "{} != {}",
            got.position,
            want.position
        );
        assert!(got.orientation.abs_diff_eq(want.orientation, 1e-4));
    }
}

/// 練習用のウィンドウの的は、それぞれのボタンでだけ押せる（左の的を右クリックしても印は付かない）。
#[gpui::test]
fn practice_targets_take_their_own_mouse_button(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, MouseDownEvent, MouseUpEvent};
    let (practice, cx) = cx.add_window_view(|_, _| Practice {
        dashboard: WeakEntity::new_invalid(),
        clicked: [false; 3],
        scrolled: false,
    });
    cx.run_until_parked();
    fn press(cx: &mut gpui::VisualTestContext, selector: &'static str, button: MouseButton) {
        let position = cx.debug_bounds(selector).unwrap().center();
        cx.simulate_event(MouseDownEvent {
            button,
            position,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            button,
            position,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        cx.run_until_parked();
    }
    press(cx, "practice-left", MouseButton::Right);
    press(cx, "practice-right", MouseButton::Right);
    press(cx, "practice-middle", MouseButton::Middle);
    assert_eq!(
        practice.read_with(cx, |p, _| p.clicked),
        [false, true, true]
    );
    press(cx, "practice-left", MouseButton::Left);
    assert_eq!(practice.read_with(cx, |p, _| p.clicked), [true; 3]);
}
