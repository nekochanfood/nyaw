//! E2E for the page stack (push transition and swipe back) on the NavTest screen and in the Settings content, with the VR laser.
//! Pressing an item slides the next page in from the right; holding the trigger and moving from right to left goes back.
//! The hand that holds (grips) the panel is ignored: only the other hand's laser can swipe.

use gpui::{Render, TestAppContext};
use kernel::{PointerMode, Setting};
use testing::{BACKGROUND, NavPage, NavTest, SIDEBAR_SELECTED_BG, Settings};
use testkit::{Action, Driver, Hand, Vec2, VrKit};

/// Holds the trigger near the right end of `selector`'s row and moves left by a bit more than half the page width, then releases.
fn swipe_back<V: Render>(kit: &mut VrKit<V>, page: &'static str, selector: &'static str) {
    let b = kit.bounds(page);
    let width = f32::from(b.size.width);
    let c = kit.bounds(selector).center();
    let from = Vec2::new(f32::from(b.origin.x) + width - 30.0, f32::from(c.y));
    kit.point_at(from).press(Action::Select);
    for i in 1..=20 {
        kit.point_at(from - Vec2::new(width * 0.6 * i as f32 / 20.0, 0.0));
    }
    kit.release(Action::Select).step();
}

fn stack(kit: &mut VrKit<NavTest>) -> Vec<NavPage> {
    kit.read(|v| v.stack.clone())
}

#[gpui::test]
fn items_push_pages_and_the_back_button_pops(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| NavTest::new(0));
    kit.click("nav-item-1");
    assert_eq!(stack(&mut kit), [NavPage::Root, NavPage::Detail(1)]);
    kit.click("nav-deeper");
    assert_eq!(stack(&mut kit).len(), 3);
    kit.click("nav-back");
    // The back button vanishes with its page while pointed at, so its tooltip must go too.
    kit.hover("nav-back").step();
    let tip =
        |kit: &mut VrKit<NavTest>| kit.panel_control(|c| c.tooltip.get(&Hand::Right).cloned());
    assert!(tip(&mut kit).is_some(), "the back button has a tooltip");
    kit.click("nav-back").step().step();
    assert_eq!(stack(&mut kit), [NavPage::Root]);
    assert_eq!(tip(&mut kit), None, "the tooltip left with the button");
    assert_eq!(kit.read(|v| v.swiped_back), 0);
}

#[gpui::test]
fn laser_swipe_goes_back_without_pressing_what_is_under_it(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| NavTest::new(0));
    kit.click("nav-item-0");
    swipe_back(&mut kit, "nav", "nav-tap");
    assert_eq!(stack(&mut kit), [NavPage::Root]);
    assert_eq!(kit.read(|v| (v.swiped_back, v.taps)), (1, 0));
}

#[gpui::test]
fn touch_long_press_then_swipe_goes_back(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| NavTest::new(0));
    kit.kernel.set(Setting::PointerMode(PointerMode::Touch));
    kit.step();
    kit.click("nav-item-0");
    assert_eq!(stack(&mut kit).len(), 2);
    // Holding still past the long press turns the touch into a mouse press; then moving left swipes back.
    let b = kit.bounds("nav");
    let width = f32::from(b.size.width);
    let c = kit.bounds("nav-tap").center();
    let from = Vec2::new(f32::from(b.origin.x) + width - 30.0, f32::from(c.y));
    kit.point_at(from).press(Action::Select);
    std::thread::sleep(ui::LONG_PRESS + std::time::Duration::from_millis(50));
    kit.step().step();
    for i in 1..=20 {
        kit.point_at(from - Vec2::new(width * 0.6 * i as f32 / 20.0, 0.0));
    }
    kit.release(Action::Select).step();
    assert_eq!(stack(&mut kit), [NavPage::Root]);
    assert_eq!(kit.read(|v| (v.swiped_back, v.taps)), (1, 0));
}

#[gpui::test]
fn touch_swipe_goes_back(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| NavTest::new(0));
    kit.kernel.set(Setting::PointerMode(PointerMode::Touch));
    kit.step();
    kit.click("nav-item-2").click("nav-deeper");
    assert_eq!(stack(&mut kit).len(), 3);
    swipe_back(&mut kit, "nav", "nav");
    assert_eq!(stack(&mut kit), [NavPage::Root, NavPage::Detail(2)]);
    // Sliding the slider's handle to the left moves the slider, not the page.
    let before = kit.read(|v| v.value);
    let h = kit.bounds("nav-slider-handle").center();
    let from = Vec2::new(f32::from(h.x), f32::from(h.y));
    kit.point_at(from).press(Action::Select);
    for i in 1..=10 {
        kit.point_at(from - Vec2::new(10.0 * i as f32, 0.0));
    }
    kit.release(Action::Select).step();
    assert_eq!(stack(&mut kit).len(), 2);
    assert!(kit.read(|v| v.value) < before);
}

#[gpui::test]
fn the_hand_holding_the_panel_is_ignored(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| NavTest::new(0));
    kit.click("nav-item-0");
    // The left hand grips the panel, then pulls the trigger and sweeps left (that scales the held panel).
    let tap = kit.bounds("nav-tap").center();
    kit.hand(Hand::Left)
        .point_at(Vec2::new(f32::from(tap.x), f32::from(tap.y)))
        .press(Action::Grab)
        .press(Action::Select);
    let mut aim = kit
        .kernel
        .subsystems
        .backend
        .frame
        .controllers
        .iter()
        .find(|c| c.hand == Hand::Left)
        .expect("left hand")
        .aim;
    for _ in 0..20 {
        aim.position.x -= 0.02;
        kit.move_hand(aim);
    }
    kit.release(Action::Select).step();
    assert_eq!(stack(&mut kit).len(), 2, "the holding hand does not swipe");
    assert!(
        kit.kernel.panel(0).pose.position.x < -0.1,
        "it moved the panel"
    );

    // While the left hand keeps holding, the right hand can still swipe back.
    kit.hand(Hand::Right);
    swipe_back(&mut kit, "nav", "nav-tap");
    assert_eq!(stack(&mut kit), [NavPage::Root]);
    assert_eq!(kit.read(|v| (v.swiped_back, v.taps)), (1, 0));
    kit.hand(Hand::Left).release(Action::Grab);
}

#[gpui::test]
fn settings_content_pushes_details_and_swipes_back(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.click("setting-1");
    assert_eq!(kit.read(|v| v.pages.clone()), [None, Some(1)]);
    swipe_back(&mut kit, "settings-content", "setting-detail-0");
    assert_eq!(kit.read(|v| v.pages.clone()), [None]);
    kit.click("setting-0").click("settings-back");
    assert_eq!(kit.read(|v| v.pages.clone()), [None]);
    // Switching category starts again from its list.
    kit.click("setting-2").click("settings-account");
    assert_eq!(kit.read(|v| (v.category, v.pages.clone())), (1, vec![None]));
}

#[gpui::test]
fn settings_sidebar_colors_only_the_selected_category(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.hover("setting-0");
    assert_eq!(kit.color_at("settings-general"), SIDEBAR_SELECTED_BG);
    assert_eq!(
        kit.color_at("settings-privacy"),
        BACKGROUND,
        "no background"
    );
    kit.click("settings-privacy").hover("setting-0");
    assert_eq!(kit.color_at("settings-privacy"), SIDEBAR_SELECTED_BG);
    assert_eq!(kit.color_at("settings-general"), BACKGROUND);
}

#[gpui::test]
fn touch_swipe_goes_back_with_the_default_smoothing(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| NavTest::new(0));
    kit.kernel.set(Setting::PointerMode(PointerMode::Touch));
    for hold in [false, true] {
        // Pressing the item without smoothing (the lag would miss it); the swipe runs with the production smoothing.
        kit.kernel.set(Setting::PointerSmoothing(0.0));
        kit.click("nav-item-0");
        kit.kernel.set(Setting::PointerSmoothing(0.998));
        kit.step().step();
        assert_eq!(stack(&mut kit).len(), 2, "pushed (hold {hold})");
        let b = kit.bounds("nav");
        let width = f32::from(b.size.width);
        let from = Vec2::new(
            f32::from(b.origin.x) + width - 30.0,
            f32::from(b.center().y),
        );
        kit.point_at(from).press(Action::Select);
        if hold {
            std::thread::sleep(ui::LONG_PRESS + std::time::Duration::from_millis(50));
            kit.step().step();
        }
        for i in 1..=20 {
            kit.point_at(from - Vec2::new(width * 0.6 * i as f32 / 20.0, 0.0));
        }
        kit.release(Action::Select).step().step();
        assert_eq!(stack(&mut kit), [NavPage::Root], "hold {hold}");
    }
}
