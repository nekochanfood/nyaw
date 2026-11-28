//! VR-specific operation scenarios: controller models, left/right hands, and grab-moving panels.
//! Basic operations are verified across all runtimes in testing.rs.

use gpui::TestAppContext;
use kernel::desktop::{Button, Frame, Status, Update, mock};
use kernel::{PanelCommand, PointerMode, Setting};
use std::sync::Arc;
use testing::Testing;
use testing::{BUTTON_ACTIVE_BG, BUTTON_BG};
use testkit::{Action, Driver, Hand, Vec2, Vec3, VrKit};

const TOUCH: &str = "oculus/touch_controller";
/// The buttons a thumb can rest on, per hand of a Touch controller.
const THUMB_TOUCHES: [(Hand, [&str; 2]); 2] = [
    (Hand::Left, ["x/touch", "y/touch"]),
    (Hand::Right, ["a/touch", "b/touch"]),
];

#[gpui::test]
fn every_controller_can_click(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    let profiles = [
        "khr/simple_controller",
        "valve/index_controller",
        "oculus/touch_controller",
        "htc/vive_controller",
        "acme/unknown_controller",
    ];
    for p in profiles {
        kit.profile(p).click("click-me");
    }
    assert_eq!(kit.read(|h| h.clicks), profiles.len());
}

#[gpui::test]
fn left_hand_clicks(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.hand(Hand::Left)
        .profile("oculus/touch_controller")
        .click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn dragged_panel_moves_in_vr_and_stays_clickable(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    let to = Vec3::new(0.3, 0.2, -1.2);
    kit.drag_panel(to);
    assert!(kit.panel_pose().position.abs_diff_eq(to, 1e-4));
    assert!(
        kit.kernel.subsystems.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq(to, 1e-4)
    );

    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

/// While the menu button is held, the Testing display lights up.
#[gpui::test]
fn menu_button_shows_on_home(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.hover("click-me").press(Action::Menu);
    assert_eq!(kit.color_at("menu-held"), BUTTON_ACTIVE_BG);
    kit.release(Action::Menu);
    assert_eq!(kit.color_at("menu-held"), BUTTON_BG);
}

/// While the panel is grabbed, the Testing display lights up.
#[gpui::test]
fn grabbing_panel_shows_on_home(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    let (w, h) = kit.kernel.panel(0).resolution;
    kit.point_at(Vec2::new(w as f32, h as f32) / 2.0)
        .press(Action::Grab);
    assert_eq!(kit.color_at("grabbed"), BUTTON_ACTIVE_BG);
    kit.release(Action::Grab);
    assert_eq!(kit.color_at("grabbed"), BUTTON_BG);
}

/// A thumb resting on a face button turns the trigger into a right or middle click for mirrored screens. nyaw's own
/// panels must still be clickable that way.
#[gpui::test]
fn resting_thumb_does_not_swallow_ui_clicks(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    let mut clicks = 0;
    for mode in [PointerMode::Mouse, PointerMode::Touch] {
        kit.kernel.set(Setting::PointerMode(mode));
        for (hand, touches) in THUMB_TOUCHES {
            kit.hand(hand).profile(TOUCH);
            for touch in touches {
                kit.input(touch, 1.0).click("click-me").input(touch, 0.0);
                clicks += 1;
                assert_eq!(kit.read(|h| h.clicks), clicks, "{mode:?} {touch}");
            }
        }
    }
}

#[gpui::test]
fn thumb_moving_during_a_press_keeps_the_click(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.profile(TOUCH).hover("click-me");
    kit.press(Action::Select)
        .input("a/touch", 1.0)
        .release(Action::Select);
    assert_eq!(kit.read(|h| h.clicks), 1, "touched while pressed");
    kit.press(Action::Select)
        .input("a/touch", 0.0)
        .release(Action::Select);
    assert_eq!(kit.read(|h| h.clicks), 2, "lifted while pressed");
}

/// Only the analog pull is reported (no click from the runtime).
#[gpui::test]
fn trigger_pull_clicks_once_past_the_threshold(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.profile(TOUCH).hover("click-me");
    for pull in [0.3, 0.55, 0.0] {
        kit.input("trigger/value", pull);
    }
    assert_eq!(kit.read(|h| h.clicks), 0, "too shallow");
    // Wavering around the press threshold stays one press.
    for pull in [0.3, 0.65, 0.5, 0.7, 0.45, 0.3, 0.0] {
        kit.input("trigger/value", pull);
    }
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn panels_that_take_mouse_buttons_get_the_right_click(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.kernel.apply(0, PanelCommand::MouseButtons);
    kit.profile(TOUCH).input("a/touch", 1.0).click("click-me");
    assert_eq!(
        kit.read(|h| h.clicks),
        0,
        "a right click is not a left click"
    );
    kit.input("a/touch", 0.0).click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn mirrored_screens_get_the_right_click(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    let (width, height) = kit.kernel.panel(0).resolution;
    let (stream, control) = mock::stream();
    kit.kernel.set_desktop_stream(0, stream).unwrap();
    // The panel takes the frame's size, so a frame of another shape would move the button's position.
    let pixels = vec![80; width as usize * height as usize * 4];
    control.publish(Update {
        status: Status::Streaming,
        frame: Some(Arc::new(Frame::new(width, height, pixels).unwrap())),
    });
    kit.step()
        .profile(TOUCH)
        .input("a/touch", 1.0)
        .click("click-me");
    let buttons = control.buttons();
    assert!(!buttons.is_empty());
    assert!(buttons.iter().all(|(_, b)| *b == Button::Right));
}
