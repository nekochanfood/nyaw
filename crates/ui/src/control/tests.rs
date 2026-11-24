use super::*;
use crate::test_util::*;
use kernel::vr::{Quat, RawController, Vec3, mock::MockBackend};

/// 右手で頭（原点）から `to` 方向を照準し、掴み・解放を 60 tick 継続する。
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

/// ドック中の画面を手で引き抜き、再び近づけて離すとドック状態の記録（[`docked`]）が追従し、画面から着脱音が鳴る。
/// [`dock`] で装着したとき（バーのボタンによる切り替え）は鳴らない。
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
    // ドック状態の記録と、反映前に画面へキューイングされた音。
    let sync = |kernel: &mut Kernel<MockBackend>, cx: &mut TestAppContext| {
        cx.update(|cx| {
            sync_docking(kernel, cx);
            let queued = &cx.default_global::<PanelControl>().panels[&screen].commands;
            let sounds: Vec<_> = queued
                .iter()
                .filter(|c| matches!(c, PanelCommand::Play(..)))
                .cloned()
                .collect();
            for p in [bar, screen] {
                sync_panel(kernel, p, cx);
            }
            (docked(cx), sounds)
        })
    };
    let sound = |wav| vec![PanelCommand::Play(kernel::audio::Sound::Wav(wav), false)];
    cx.update(|cx| {
        offer_dock(bar, 0.0, cx);
        dock(screen, cx);
    });
    assert_eq!(sync(&mut kernel, cx), (Some(screen), vec![]));
    kernel.tick();
    let slot = kernel.panel(screen).pose.position;
    let away = slot + Vec3::X * 0.5;
    hand(&mut kernel, slot, true);
    hand(&mut kernel, away, true);
    hand(&mut kernel, away, false);
    assert_eq!(kernel.panel(screen).parent, None);
    assert_eq!(sync(&mut kernel, cx), (None, sound(UNDOCK_SOUND)));
    hand(&mut kernel, away, true);
    hand(&mut kernel, slot, true);
    hand(&mut kernel, slot, false);
    assert_eq!(kernel.panel(screen).parent.map(|p| p.panel), Some(bar));
    assert_eq!(sync(&mut kernel, cx), (Some(screen), sound(DOCK_SOUND)));
}
