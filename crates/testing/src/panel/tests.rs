use super::*;
use crate::test_util::*;

/// ミラーリング画面の確認用: 操作のパネルで選ぶと、表示のパネルが描き直しを頼まれ、選んだ状態の表示になる。
/// どの状態でも表示が出て、止まった状態（最小化・閉じた・失敗）のときだけボタン（戻す・再試行）が出る。
#[gpui::test]
fn mirror_test_controls_the_preview_panel(cx: &mut TestAppContext) {
    let shared = Rc::new(MirrorShared::default());
    let (_, cx) = cx.add_window_view(|_, _| MirrorTest {
        shared: shared.clone(),
    });
    cx.run_until_parked();
    let ids = [
        ("mirror-case-launching", false),
        ("mirror-case-launch-failed", false),
        ("mirror-case-starting", false),
        ("mirror-case-streaming", false),
        ("mirror-case-minimized", true),
        ("mirror-case-closed", true),
        ("mirror-case-failed-windowunavailable", true),
        ("mirror-case-failed-unsupported", true),
        ("mirror-case-failed-nopixels", true),
        ("mirror-case-failed-invalidbounds", true),
        ("mirror-case-failed-graphics", true),
        ("mirror-case-failed-other", true),
    ];
    let mut seen = Vec::new();
    for (id, _) in ids {
        let c = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("{id}"))
            .center();
        cx.simulate_click(c, Modifiers::none());
        cx.run_until_parked();
        seen.push(shared.shown());
    }
    // 全部違う状態になっている。
    for (i, a) in seen.iter().enumerate() {
        assert!(seen[i + 1..].iter().all(|b| a != b), "{}", ids[i].0);
    }
    // 表示側: 各状態で画面が出て、再試行は止まった状態のときだけ。
    let (_, cx) = cx.add_window_view(|_, _| MirrorPreview {
        shared: shared.clone(),
    });
    for (state, (id, retry)) in seen.into_iter().zip(ids) {
        *shared.state.borrow_mut() = Some(state);
        cx.update(|w, _| w.refresh());
        cx.run_until_parked();
        assert!(cx.debug_bounds("mirror-test-screen").is_some(), "{id}");
        assert_eq!(
            cx.debug_bounds("mirror-test-action").is_some(),
            retry,
            "{id}"
        );
    }
}

/// ボタンは操作要求を積むだけで、反映はフレームループ（`ui::sync_panel`）の仕事。
#[gpui::test]
fn attach_button_queues_command(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Testing::default());
    cx.run_until_parked();
    let c = cx.debug_bounds("attach-hmd").unwrap().center();
    cx.simulate_click(c, Modifiers::none());
    let queued = cx.update(|_, cx| {
        cx.global::<PanelControl>().panels[&PanelId::default()]
            .commands
            .clone()
    });
    assert_eq!(queued, [PanelCommand::Attach(Device::Hmd, HMD_OFFSET)]);
}

/// テーマボタンは全体のテーマを切り替える要求を積む。切り替わると画面の色も変わる。
#[gpui::test]
fn theme_button_requests_toggle_and_screen_follows(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Testing::default());
    cx.run_until_parked();
    let c = cx.debug_bounds("toggle-theme").unwrap().center();
    cx.simulate_click(c, Modifiers::none());
    let queued = cx.update(|_, cx| cx.global::<PanelControl>().setting_changes.clone());
    assert_eq!(queued, [Setting::Theme(kernel::ThemeMode::Light)]);
    cx.update(|_, cx| cx.set_global(Mode::Light));
    cx.run_until_parked();
    assert!(cx.update(|_, cx| Theme::current(cx)) == Theme::LIGHT);
}

/// 言語ボタンは次の言語へ切り替える要求を積む（設定ファイルへの書き戻しはカーネルの仕事）。
#[gpui::test]
fn language_button_requests_the_next_language(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Testing::default());
    cx.run_until_parked();
    let c = cx.debug_bounds("toggle-language").unwrap().center();
    cx.simulate_click(c, Modifiers::none());
    let queued = cx.update(|_, cx| cx.global::<PanelControl>().setting_changes.clone());
    assert_eq!(queued, [Setting::Language(Language::Ja)]);
}

/// Profiler ボタンは計測の有効・無効を切り替える。
#[gpui::test]
fn profiler_button_toggles_profiling(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Testing::default());
    cx.run_until_parked();
    let profiling =
        |cx: &mut gpui::VisualTestContext| cx.update(|_, cx| cx.global::<PanelControl>().profiling);
    let c = cx.debug_bounds("toggle-profiler").unwrap().center();
    cx.simulate_click(c, Modifiers::none());
    assert!(profiling(cx));
    cx.simulate_click(c, Modifiers::none());
    assert!(!profiling(cx));
}

/// 描画のテストパネルと負荷試験の画面は Testing から召喚できる。
#[gpui::test]
fn test_panel_buttons_spawn_panels(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Testing::default());
    cx.run_until_parked();
    for id in [
        "spawn-anim",
        "spawn-lod",
        "spawn-font",
        "stress-test",
        "keyboard-test",
    ] {
        let c = cx.debug_bounds(id).unwrap().center();
        cx.simulate_click(c, Modifiers::none());
    }
    let spawns = cx.update(|_, cx| cx.global::<PanelControl>().spawns.len());
    assert_eq!(spawns, 5);
}

#[gpui::test]
fn keyboard_test_opens_in_front_of_the_view_instead_of_inheriting_the_source_tilt(
    cx: &mut TestAppContext,
) {
    let mut kernel =
        kernel::Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    kernel.subsystems.backend.frame.hmd =
        Pose::new(Vec3::new(2., 1., 3.), Quat::from_rotation_y(0.8));
    kernel.tick();
    let source = kernel
        .add_panel(
            "source",
            Pose::new(Vec3::new(5., -2., -4.), Quat::from_rotation_z(1.)),
            TESTING_SIZE,
            TESTING_PX,
        )
        .unwrap();
    let (_, cx) = cx.add_window_view(|_, _| Testing::new(source));
    cx.run_until_parked();
    let at = cx.debug_bounds("keyboard-test").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    cx.update(|_, cx| {
        let spec = cx.default_global::<PanelControl>().spawns.pop().unwrap();
        let pose = spec.pose_for(&kernel, 1);
        assert_eq!(
            pose,
            kernel.hmd().leveled() * Pose::new(Vec3::new(0., 0.23, -0.85), Quat::IDENTITY)
        );
        assert!(spec.parent().is_none());
    });
}
