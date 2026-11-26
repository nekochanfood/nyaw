use super::*;
use crate::test_util::*;

#[gpui::test]
fn laser_click_reaches_gpui_view(cx: &mut TestAppContext) {
    let (testing, cx) = cx.add_window_view(|_, _| Probe::default());
    cx.run_until_parked();
    let c = cx.debug_bounds("probe").expect("button rendered").center();
    let pos = Vec2::new(c.x.into(), c.y.into());

    let mut bridge = PointerBridge::default();
    for ev in [
        PointerEvent::Move { pos },
        PointerEvent::Down { pos },
        PointerEvent::Up { pos },
        PointerEvent::Leave,
    ] {
        for input in bridge.translate(ev, PointerMode::Mouse) {
            cx.update(|window, cx| {
                window.dispatch_event(input, cx);
            });
        }
    }
    cx.run_until_parked();
    assert_eq!(testing.read_with(cx, |h, _| h.clicks), 1);
}

#[gpui::test]
fn sync_panel_applies_commands_and_reports_status(cx: &mut TestAppContext) {
    let mut kernel = Kernel::boot(Some("novr"), Default::default(), false).unwrap();
    let panel = kernel
        .add_panel("t", Pose::IDENTITY, Vec2::ONE, PANEL_PX)
        .unwrap();
    cx.update(|cx| {
        commands(
            panel,
            [
                PanelCommand::Attach(Device::Hmd, Pose::IDENTITY),
                PanelCommand::Lock(true),
            ],
            cx,
        );
        sync_panel(&mut kernel, panel, cx);
        let ctl = &cx.global::<PanelControl>().panels[&panel];
        assert!(ctl.commands.is_empty());
        assert_eq!(
            ctl.status,
            PanelStatus {
                runtime: Some(Runtime::NoVr),
                attached: Some(Device::Hmd),
                locked: true,
                animated: true,
                ..Default::default()
            }
        );
        assert_eq!(Theme::current(cx), Theme::DARK);
        set(Setting::Theme(ThemeMode::Light), cx);
        sync_panel(&mut kernel, panel, cx);
        assert_eq!(Theme::current(cx), Theme::LIGHT, "every screen follows");
        let color = kernel::PersonalColor {
            lightness: 0.87,
            chroma: 0.18,
            hue: 95.0,
        };
        set(Setting::PersonalColor(color), cx);
        sync_panel(&mut kernel, panel, cx);
        let yellow = design::Accent::new(0.87, 0.18, 95.0);
        let now = Theme::current(cx);
        assert_eq!(now, Theme::with(design::Mode::Light, yellow));
        assert_eq!(
            now.on_accent,
            design::scale::BLACK,
            "dark text on a light fill"
        );
        commands(
            panel,
            [PanelCommand::Detach, PanelCommand::Animated(false)],
            cx,
        );
        sync_panel(&mut kernel, panel, cx);
    });
    assert_eq!(kernel.panel(panel).attached, None);
    assert!(!kernel.panel(panel).animated);
}
