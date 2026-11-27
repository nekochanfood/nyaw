//! Controller guide: posing as a specific controller (the render model name SteamVR reports) is detected per hand,
//! the guide screen shows that controller in use, and hovering a binding emphasizes it.

use gpui::TestAppContext;
use guide::Model;
use testing::{BUTTON_ACTIVE_BG, BUTTON_BG, GuideTest};
use testkit::{Driver, Hand, VrKit};

#[gpui::test]
fn posed_controllers_are_detected_and_shown(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| GuideTest::new(0, cx));
    assert_eq!(kit.color_at("guide-detected-right"), BUTTON_BG);
    // Nothing in hand: the guide falls back to the Quest Touch Plus and says so.
    assert!(kit.try_bounds("guide-note").is_some());
    let fallback = kit.bounds("guide-grab-ring");

    // Right hand: a Quest 2 controller through SteamVR's Oculus driver. Left hand: an Index controller.
    kit.hand(Hand::Right)
        .profile("oculus/touch_controller")
        .render_model("{oculus}oculus_quest2_controller_right");
    kit.hand(Hand::Left)
        .profile("valve/index_controller")
        .render_model("{indexcontroller}valve_controller_knu_1_0_left");
    kit.step().step();
    let detected = |kit: &VrKit<GuideTest>, hand| {
        kit.kernel
            .controllers()
            .iter()
            .find(|c| c.hand == hand)
            .and_then(|c| c.controller)
            .map(|c| c.model)
    };
    assert_eq!(detected(&kit, Hand::Right), Some(Model::Quest2));
    assert_eq!(detected(&kit, Hand::Left), Some(Model::Index));
    assert_eq!(kit.color_at("guide-detected-right"), BUTTON_ACTIVE_BG);
    assert_eq!(kit.color_at("guide-detected-left"), BUTTON_ACTIVE_BG);
    assert!(kit.try_bounds("guide-note").is_none());

    // "In use" draws the Quest 2: its grip sits where the Quest 2 entry of the list puts it, not where the fallback did.
    let in_use = kit.bounds("guide-grab-ring");
    assert_ne!(in_use, fallback);
    kit.click("guide-model-quest-2");
    assert_eq!(kit.bounds("guide-grab-ring"), in_use);
    kit.click("guide-in-use");

    // The trigger is behind the controller: it gets a label and a tail, but no ring.
    assert!(kit.try_bounds("guide-select").is_some());
    assert!(kit.try_bounds("guide-select-ring").is_none());
    // The Quest 2 has no shortcut on the right hand (X is on the left).
    assert!(kit.try_bounds("guide-shortcut").is_none());

    // Hovering a binding emphasizes it; moving away clears it.
    kit.hover("guide-bind-grab");
    assert_eq!(kit.read(|v| v.hovered), Some(1));
    assert_eq!(kit.color_at("guide-bind-grab"), BUTTON_ACTIVE_BG);
    kit.hover("guide-bind-menu");
    assert_eq!(kit.read(|v| v.hovered), Some(2));
    assert_eq!(kit.color_at("guide-bind-grab"), BUTTON_BG);

    // The left hand in use is the Index: its shortcut is the A button, ringed.
    kit.click("guide-hand-left");
    assert!(kit.try_bounds("guide-shortcut-ring").is_some());

    // A controller with an unknown render model is shown as the Quest Touch Plus.
    kit.hand(Hand::Right)
        .profile("khr/simple_controller")
        .render_model("acme_wand_right");
    kit.step().step();
    assert_eq!(detected(&kit, Hand::Right), Some(Model::QuestPlus));
}
