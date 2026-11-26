use super::*;
use crate::test_util::*;

/// `panel` にキューイングされた操作を取り出す。
fn take(panel: PanelId, cx: &mut App) -> Vec<PanelCommand> {
    let ctl = cx.default_global::<PanelControl>();
    std::mem::take(&mut ctl.panels.entry(panel).or_default().commands)
}

/// 手に保持中のパネルは、ダッシュボード非表示時のみグラブロックする（変更時のみキューイング）。手から外れると解除する。
#[gpui::test]
fn a_panel_in_hand_is_grabbable_only_while_the_dashboard_shows(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut locked = false;
        let mut step = |cx: &mut App, f: &dyn Fn(&mut PanelControl)| {
            f(cx.default_global::<PanelControl>());
            hold(1, &mut locked, cx);
            take(1, cx)
        };
        let attach = |device: Option<Device>| {
            move |ctl: &mut PanelControl| {
                ctl.panels.entry(1).or_default().status.attached = device.clone()
            }
        };
        let hidden =
            |on| move |ctl: &mut PanelControl| ctl.panels.entry(5).or_default().status.hidden = on;
        assert_eq!(step(cx, &|_| {}), []);
        // HMD に装着されたものは手として扱わない。
        assert_eq!(step(cx, &attach(Some(Device::Hmd))), []);
        let left = Some(Device::Controller(Hand::Left));
        assert_eq!(step(cx, &attach(left.clone())), [PanelCommand::Lock(true)]);
        assert_eq!(step(cx, &|_| {}), []);
        // ダッシュボード（ドック持ち主 5）が表示されるとグラブ可能になり、非表示で再ロックする。
        assert_eq!(
            step(cx, &|ctl| ctl.dock = Some((5, 0.0))),
            [PanelCommand::Lock(false)]
        );
        assert_eq!(step(cx, &hidden(true)), [PanelCommand::Lock(true)]);
        // 手から外れるとロックを解除する。
        assert_eq!(step(cx, &attach(None)), [PanelCommand::Lock(false)]);
        assert_eq!(step(cx, &hidden(false)), []);
    });
}

/// 手を選択すると持ち主をその手に装着し、ポップオーバーは閉じる。ドック中の画面は取り外して独立させてから装着する。
/// 未保持時は「手から外す」は表示されない。
#[gpui::test]
fn choosing_a_hand_holds_the_owner_in_it(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, cx| {
        let state = cx
            .default_global::<PanelControl>()
            .panels
            .entry(1)
            .or_default();
        state.status.parent = Some(0);
        NavMenu {
            panel: 3,
            owner: 1,
            hand: None,
            docked: None,
        }
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("hand-detach").is_none());
    let left = cx.debug_bounds("hand-left").unwrap();
    cx.simulate_click(left.center(), gpui::Modifiers::none());
    assert_eq!(
        cx.update(|_, cx| take(1, cx)),
        [
            PanelCommand::Parent(None),
            PanelCommand::AttachToHand(Hand::Left)
        ]
    );
    assert_eq!(cx.update(|_, cx| take(3, cx)), [PanelCommand::Close]);
}

/// 装着中の手を再選択しても何もしない（ポップオーバーを閉じるのみ）。
#[gpui::test]
fn choosing_the_held_hand_again_changes_nothing(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| NavMenu {
        panel: 4,
        owner: 1,
        hand: Some(Hand::Right),
        docked: None,
    });
    cx.run_until_parked();
    let right = cx.debug_bounds("hand-right").unwrap();
    cx.simulate_click(right.center(), gpui::Modifiers::none());
    assert_eq!(cx.update(|_, cx| take(1, cx)), []);
    assert_eq!(cx.update(|_, cx| take(4, cx)), [PanelCommand::Close]);
}

/// 「手から外す」は持ち主をその場に残して手から取り外し、ポップオーバーを閉じる。
#[gpui::test]
fn letting_go_detaches_the_owner(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| NavMenu {
        panel: 3,
        owner: 1,
        hand: Some(Hand::Left),
        docked: None,
    });
    cx.run_until_parked();
    let detach = cx.debug_bounds("hand-detach").unwrap();
    cx.simulate_click(detach.center(), gpui::Modifiers::none());
    assert_eq!(cx.update(|_, cx| take(1, cx)), [PanelCommand::Detach]);
    assert_eq!(cx.update(|_, cx| take(3, cx)), [PanelCommand::Close]);
}

/// ドック中であれば「ダッシュボードから外す」、未装着でドックが空きなら「戻す」を表示し、選択で着脱音とともに実行して閉じる。
/// ドック非対応パネルやドック非表示時は表示しない。
#[gpui::test]
fn the_menu_docks_and_undocks_the_owner(cx: &mut TestAppContext) {
    let rows =
        |cx: &mut TestAppContext, dockable| cx.update(|cx| menu_spec(1, dockable, cx)).resolution.1;
    let two = rows(cx, true);
    cx.update(|cx| cx.default_global::<PanelControl>().dock = Some((5, 0.0)));
    assert_eq!(rows(cx, false), two, "only for a dockable panel");
    let three = rows(cx, true);
    assert_eq!(
        three - two,
        design::components::MENU_ITEM_HEIGHT as u32,
        "one more row"
    );
    cx.update(|cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(1)
            .or_default()
            .status
            .parent = Some(5)
    });
    assert_eq!(cx.update(|cx| dock_state(1, cx)), Some(true));

    let menu = |docked| {
        move |_: &mut Window, _: &mut Context<NavMenu>| NavMenu {
            panel: 3,
            owner: 1,
            hand: None,
            docked: Some(docked),
        }
    };
    let (_, undock) = cx.add_window_view(menu(true));
    undock.run_until_parked();
    assert!(undock.debug_bounds("nav-dock").is_none());
    let row = undock.debug_bounds("nav-undock").unwrap();
    undock.simulate_click(row.center(), gpui::Modifiers::none());
    let sound = |wav| PanelCommand::Play(kernel::audio::Sound::Wav(wav), false);
    assert_eq!(
        undock.update(|_, cx| take(1, cx)),
        [PanelCommand::Parent(None), sound(UNDOCK_SOUND)]
    );
    assert_eq!(undock.update(|_, cx| take(3, cx)), [PanelCommand::Close]);

    let (_, dock) = cx.add_window_view(menu(false));
    dock.run_until_parked();
    let row = dock.debug_bounds("nav-dock").unwrap();
    dock.simulate_click(row.center(), gpui::Modifiers::none());
    assert!(matches!(
        dock.update(|_, cx| take(1, cx)).as_slice(),
        [PanelCommand::Parent(Some(p)), s] if p.panel == 5 && p.docked && *s == sound(DOCK_SOUND)
    ));
    assert_eq!(dock.update(|_, cx| take(3, cx)), [PanelCommand::Close]);
}

/// ポップオーバーはナビゲーションバー上に、カード右端をバーの帯の右端（「…」側）に揃えて表示する。
/// バーは頭からの距離に比例して拡大するため、ポップオーバーも同じ比率で拡大する（見た目の比率が一定）。
#[gpui::test]
fn the_menu_sits_over_the_nav_bar_at_its_scale(cx: &mut TestAppContext) {
    let mut kernel =
        Kernel::with_backend(kernel::vr::mock::MockBackend::default(), Default::default());
    let owner = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
    kernel
        .add_panel("owner", owner, Vec2::new(0.8, 0.6), (800, 600))
        .unwrap();
    let bar = kernel
        .add_panel(
            "bar",
            Pose::IDENTITY,
            Vec2::new(NAV_BAR_M, NAV_BAR_HEIGHT_M),
            NAV_BAR_PX,
        )
        .unwrap();
    kernel.apply(bar, PanelCommand::NavBar(0));
    let margin = design::components::PANEL_MARGIN;
    let place = |kernel: &mut Kernel<_>, cx: &mut TestAppContext| {
        kernel.tick();
        let spec = cx.update(|cx| menu_spec(0, false, cx));
        let (size, res) = spec.size(kernel);
        let pose = spec.pose_for(kernel, 2);
        let b = kernel.panel(bar);
        // 1px あたりの大きさ（m）、ポップオーバー・帯の右端座標、カード下端・帯上端（バー面上の座標）。
        let (m, n) = (size.x / res.0 as f32, b.size.x / NAV_BAR_PX.0 as f32);
        let local = b.pose.inverse().transform_point(pose.position);
        let card_right = local.x + size.x / 2.0 - margin * m;
        let band_right = b.size.x / 2.0 - margin * n;
        let card_bottom = local.y - size.y / 2.0 + margin * m;
        let band_top = b.size.y / 2.0 - margin * n;
        assert!(
            (card_right - band_right).abs() < 1e-5,
            "{card_right} {band_right}"
        );
        assert!(
            (card_bottom - band_top - design::spacing::SM * n).abs() < 1e-5,
            "{card_bottom} {band_top}"
        );
        // バー（と持ち主）より手前に浮かせて配置する。
        assert!(
            (local.z - MENU_LIFT * b.size.x / NAV_BAR_M).abs() < 1e-5,
            "{local}"
        );
        assert!((m - n).abs() < 1e-7, "same density as the bar");
        (size.x, b.size.x)
    };
    let near = place(&mut kernel, cx);
    kernel.apply(
        0,
        PanelCommand::Place(Pose::new(Vec3::new(0.0, 0.0, -2.0), Quat::IDENTITY)),
    );
    let far = place(&mut kernel, cx);
    assert!(far.1 > near.1 * 1.8, "the bar grows with the distance");
    assert!(
        (far.0 / near.0 - far.1 / near.1).abs() < 1e-4,
        "{near:?} {far:?}"
    );
}
