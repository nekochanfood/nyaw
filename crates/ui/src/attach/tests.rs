use super::*;
use crate::test_util::*;

/// `panel` に積まれた操作を取り出す。
fn take(panel: PanelId, cx: &mut App) -> Vec<PanelCommand> {
    let ctl = cx.default_global::<PanelControl>();
    std::mem::take(&mut ctl.panels.entry(panel).or_default().commands)
}

/// 手に持っている間だけ、ダッシュボードが出ていなければロックする（変わったときだけ積む）。手から外れたら外す。
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
        // 頭に付けたものは手ではない。
        assert_eq!(step(cx, &attach(Some(Device::Hmd))), []);
        let left = Some(Device::Controller(Hand::Left));
        assert_eq!(step(cx, &attach(left.clone())), [PanelCommand::Lock(true)]);
        assert_eq!(step(cx, &|_| {}), []);
        // ダッシュボード（はめる場所の持ち主 5）が出ると掴め、隠れるとまたロックする。
        assert_eq!(
            step(cx, &|ctl| ctl.dock = Some((5, 0.0))),
            [PanelCommand::Lock(false)]
        );
        assert_eq!(step(cx, &hidden(true)), [PanelCommand::Lock(true)]);
        // 手から外れたらロックを外す。
        assert_eq!(step(cx, &attach(None)), [PanelCommand::Lock(false)]);
        assert_eq!(step(cx, &hidden(false)), []);
    });
}

/// 手を選ぶと持ち主をその手に持ち、ポップオーバーは閉じる。はめた画面は外して独立させてから持つ。持っていなければ
/// 「手から外す」は無い。
#[gpui::test]
fn choosing_a_hand_holds_the_owner_in_it(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, cx| {
        let state = cx
            .default_global::<PanelControl>()
            .panels
            .entry(1)
            .or_default();
        state.status.parent = Some(0);
        HandMenu {
            panel: 3,
            owner: 1,
            hand: None,
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

/// 持っている手を選び直しても何もしない（閉じるだけ）。
#[gpui::test]
fn choosing_the_held_hand_again_changes_nothing(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| HandMenu {
        panel: 4,
        owner: 1,
        hand: Some(Hand::Right),
    });
    cx.run_until_parked();
    let right = cx.debug_bounds("hand-right").unwrap();
    cx.simulate_click(right.center(), gpui::Modifiers::none());
    assert_eq!(cx.update(|_, cx| take(1, cx)), []);
    assert_eq!(cx.update(|_, cx| take(4, cx)), [PanelCommand::Close]);
}

/// 「手から外す」は持ち主をその場に残して手から外し、ポップオーバーを閉じる。
#[gpui::test]
fn letting_go_detaches_the_owner(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| HandMenu {
        panel: 3,
        owner: 1,
        hand: Some(Hand::Left),
    });
    cx.run_until_parked();
    let detach = cx.debug_bounds("hand-detach").unwrap();
    cx.simulate_click(detach.center(), gpui::Modifiers::none());
    assert_eq!(cx.update(|_, cx| take(1, cx)), [PanelCommand::Detach]);
    assert_eq!(cx.update(|_, cx| take(3, cx)), [PanelCommand::Close]);
}

/// ポップオーバーはナビゲーションバーの上に、カードの左端をバーの帯の左端にそろえて出す。バーは頭からの距離で大きくなるので、
/// ポップオーバーも同じ割合で大きく・遠くなる（バーとの見た目の比が変わらない）。
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
        let spec = cx.update(|cx| menu_spec(0, cx));
        let (size, res) = spec.size(kernel);
        let pose = spec.pose_for(kernel, 2);
        let b = kernel.panel(bar);
        // 1px の大きさ（m）と、ポップオーバー・帯の左端と、カードの下端・帯の上端（バーの面の上で）。
        let (m, n) = (size.x / res.0 as f32, b.size.x / NAV_BAR_PX.0 as f32);
        let local = b.pose.inverse().transform_point(pose.position);
        let card_left = local.x - size.x / 2.0 + margin * m;
        let band_left = -b.size.x / 2.0 + margin * n;
        let card_bottom = local.y - size.y / 2.0 + margin * m;
        let band_top = b.size.y / 2.0 - margin * n;
        assert!(
            (card_left - band_left).abs() < 1e-5,
            "{card_left} {band_left}"
        );
        assert!(
            (card_bottom - band_top - design::spacing::SM * n).abs() < 1e-5,
            "{card_bottom} {band_top}"
        );
        // バー（と持ち主）より手前に浮かせる。
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
