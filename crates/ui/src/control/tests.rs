use super::*;
use crate::test_util::*;
use kernel::vr::{Quat, RawController, Vec3, mock::MockBackend};

/// 右手で頭（原点）から `to` の方を狙い、掴む・離すを 60 tick 続ける。
fn hand(kernel: &mut Kernel<MockBackend>, to: Vec3, grab: bool) {
    kernel.subsystems.backend.frame.controllers = vec![RawController {
        hand: Hand::Right,
        profile: "valve/index_controller".into(),
        aim: Pose::new(
            Vec3::ZERO,
            Quat::from_rotation_arc(Vec3::NEG_Z, to.normalize()),
        ),
        inputs: vec![("squeeze/value".into(), if grab { 1.0 } else { 0.0 })],
    }];
    for _ in 0..60 {
        kernel.tick();
    }
}

/// はめた画面を手で引っこ抜く・近づけて離してはめると、はまっている画面の記録（[`docked`]）もそれに合わせて変わる。
#[gpui::test]
fn pulling_a_screen_off_by_hand_and_docking_it_again_updates_docked(cx: &mut TestAppContext) {
    let mut kernel = Kernel::with_backend(MockBackend::default(), Default::default());
    let at = |y: f32| Pose::new(Vec3::new(0.0, y, -1.0), Quat::IDENTITY);
    let bar = kernel
        .add_panel("bar", at(-0.3), Vec2::new(1.0, 0.1), (1000, 100))
        .unwrap();
    let screen = kernel
        .add_panel("screen", at(0.0), Vec2::new(0.4, 0.3), (400, 300))
        .unwrap();
    kernel.apply(screen, PanelCommand::Animated(false));
    kernel.apply(screen, PanelCommand::Dockable(true));
    let sync = |kernel: &mut Kernel<MockBackend>, cx: &mut TestAppContext| {
        cx.update(|cx| {
            for p in [bar, screen] {
                sync_panel(kernel, p, cx);
            }
            docked(cx)
        })
    };
    cx.update(|cx| {
        offer_dock(bar, 0.0, cx);
        dock(screen, cx);
    });
    assert_eq!(sync(&mut kernel, cx), Some(screen));
    kernel.tick();
    let slot = kernel.panel(screen).pose.position;
    let away = slot + Vec3::X * 0.5;
    hand(&mut kernel, slot, true);
    hand(&mut kernel, away, true);
    hand(&mut kernel, away, false);
    assert_eq!(kernel.panel(screen).parent, None);
    assert_eq!(sync(&mut kernel, cx), None);
    hand(&mut kernel, away, true);
    hand(&mut kernel, slot, true);
    hand(&mut kernel, slot, false);
    assert_eq!(kernel.panel(screen).parent.map(|p| p.panel), Some(bar));
    assert_eq!(sync(&mut kernel, cx), Some(screen));
}
