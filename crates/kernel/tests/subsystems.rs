//! 生入力が Subsystems を通って共通のパネル操作になることを確かめる。

use kernel::{
    Config, Hand, Kernel, PanelCommand, PointerEvent, Pose, Vec2,
    vr::{Quat, RawController, Vec3, mock::MockBackend},
};

#[test]
fn controller_changes_and_disconnects_do_not_leak_pressed_state_to_panels() {
    let mut kernel = Kernel::with_backend(
        MockBackend::default(),
        Config {
            pointer_smoothing: Some(0.0),
            ..Default::default()
        },
    );
    let panel = kernel
        .add_panel(
            "input",
            Pose::new(Vec3::NEG_Z, Quat::IDENTITY),
            Vec2::ONE,
            (100, 100),
        )
        .unwrap();
    kernel.apply(panel, PanelCommand::Animated(false));
    for profile in [
        "valve/index_controller",
        "oculus/touch_controller",
        "htc/vive_controller",
    ] {
        kernel.subsystems.backend.frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: profile.into(),
            aim: Pose::IDENTITY,
            inputs: vec![("trigger/value".into(), 0.5)],
        }];
        let events = kernel.tick();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.event, PointerEvent::Down { .. }))
        );
        kernel.subsystems.backend.frame.controllers[0].inputs[0].1 = 0.7;
        let events = kernel.tick();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.panel == panel && matches!(e.event, PointerEvent::Down { .. }))
                .count(),
            1
        );
        kernel.subsystems.backend.frame.controllers[0].inputs[0].1 = 0.5;
        let events = kernel.tick();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.event, PointerEvent::Down { .. } | PointerEvent::Up { .. }))
        );
        kernel.subsystems.backend.frame.controllers.clear();
        let events = kernel.tick();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.panel == panel && matches!(e.event, PointerEvent::Leave))
                .count(),
            1
        );
    }
}
