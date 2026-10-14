use gpui::{MouseButton, PlatformInput, TestAppContext};
use kernel::{Device, Kernel};
use std::collections::HashMap;
use testing::{BUTTON_ACTIVE_BG, BUTTON_BG, Testing};
use testkit::{Driver, Hand, NoVrKit, Pose, Quat, Vec2, Vec3, VrKit};
use ui::PointerBridge;
use vr::{RawController, mock::MockBackend};

#[gpui::test]
fn panels_have_separate_views_controls_and_textures(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::new(0));
    kit.click("spawn-panel");
    let second = 1;
    assert_eq!(kit.kernel.vr.overlays.len(), 2);

    kit.panel(second)
        .click("click-me")
        .click("attach-hmd")
        // 取り付けた先へパネルが動くのを待ってから次を押す。
        .step()
        .click("lock")
        .click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 2);
    assert_eq!(kit.kernel.status(second).attached, Some(Device::Hmd));
    assert!(kit.kernel.panel(second).locked);
    assert_eq!(kit.color_at("lock"), BUTTON_ACTIVE_BG);

    kit.panel(0);
    assert_eq!(kit.read(|h| h.clicks), 0);
    assert_eq!(kit.kernel.status(0).attached, None);
    assert!(!kit.kernel.panel(0).locked);
    assert_eq!(kit.color_at("lock"), BUTTON_BG);
    assert_ne!(
        kit.kernel.vr.overlays[0].texture,
        kit.kernel.vr.overlays[1].texture
    );
}

#[gpui::test]
fn desktop_panels_can_spawn_attach_and_lock_independently(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::new(0));
    kit.click("spawn-panel");
    kit.select_panel(1)
        .click("click-me")
        .click("attach-hmd")
        .click("lock")
        .click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 2);
    assert_eq!(kit.kernel.status(1).attached, Some(Device::Hmd));
    assert!(kit.kernel.panel(1).locked);

    kit.select_panel(0);
    assert_eq!(kit.read(|h| h.clicks), 0);
    assert_eq!(kit.kernel.status(0).attached, None);
    assert!(!kit.kernel.panel(0).locked);
}

#[test]
fn hands_keep_independent_pressed_state_on_same_panel() {
    let mut kernel = Kernel::with_backend(MockBackend::default(), Default::default());
    kernel
        .add_panel(
            "shared",
            Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
            Vec2::ONE,
            (100, 100),
        )
        .unwrap();
    kernel.vr.frame.controllers = [Hand::Right, Hand::Left]
        .map(|hand| RawController {
            hand,
            profile: "valve/index_controller".into(),
            aim: Pose::IDENTITY,
            inputs: if hand == Hand::Right {
                vec![("trigger/value".into(), 1.0)]
            } else {
                vec![]
            },
        })
        .to_vec();
    let mut bridges: HashMap<_, PointerBridge> = HashMap::new();
    ui::pump(&mut kernel, &mut bridges);
    let moves: Vec<_> = ui::pump(&mut kernel, &mut bridges)
        .into_iter()
        .filter_map(|(_, input)| match input {
            PlatformInput::MouseMove(e) => Some(e.pressed_button),
            _ => None,
        })
        .collect();
    assert_eq!(moves, [Some(MouseButton::Left), None]);
}
