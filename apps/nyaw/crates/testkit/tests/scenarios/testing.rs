//! Operations on the Testing screen. Testing is a test screen that exercises a series of features on one screen,
//! and basic operations should be doable natively on every runtime, so each scenario is written once and run on all runtimes (novr / VR).

use gpui::TestAppContext;
use testing::{
    BUTTON_ACTIVE_BG, BUTTON_BG, BUTTON_HOVER_BG, DROP_TARGET_BG, FIELD_FOCUS_BG, Testing,
};
use testkit::{Driver, NoVrKit, on_every_runtime};

fn click_button(kit: &mut impl Driver<Testing>) {
    kit.click("click-me").click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 2);
}
on_every_runtime!(click_button, |_, _| Testing::default());

fn hover_highlights_button(kit: &mut impl Driver<Testing>) {
    assert_eq!(kit.color_at("click-me"), BUTTON_BG);
    kit.hover("click-me");
    assert_eq!(kit.color_at("click-me"), BUTTON_HOVER_BG);
}
on_every_runtime!(hover_highlights_button, |_, _| Testing::default());

/// Lock is toggled with a button, and while enabled it is shown by the button color.
fn lock_button_toggles(kit: &mut impl Driver<Testing>) {
    kit.click("lock").hover("click-me");
    assert_eq!(kit.color_at("lock"), BUTTON_ACTIVE_BG);
    kit.click("lock").hover("click-me");
    assert_eq!(kit.color_at("lock"), BUTTON_BG);
}
on_every_runtime!(lock_button_toggles, |_, _| Testing::default());

fn animation_button_opts_out(kit: &mut impl Driver<Testing>) {
    // The kits' defaults differ, but both toggle on the first click.
    let before = kit.color_at("animate-panel");
    kit.click("animate-panel").hover("click-me");
    let after = kit.color_at("animate-panel");
    assert_ne!(before, after);
    kit.click("animate-panel").hover("click-me");
    assert_eq!(kit.color_at("animate-panel"), before);
}
on_every_runtime!(animation_button_opts_out, |_, _| Testing::default());

/// Only one attach target lights up, and it goes out when detached.
/// (Attaching to the operating right hand makes the panel follow the laser tip so nothing else can be pointed at, so the right hand is handled from the left hand in attach.rs)
fn attach_buttons_show_target(kit: &mut impl Driver<Testing>) {
    kit.click("attach-hmd")
        .click("attach-left")
        .hover("click-me");
    assert_eq!(kit.color_at("attach-hmd"), BUTTON_BG);
    assert_eq!(kit.color_at("attach-left"), BUTTON_ACTIVE_BG);
    kit.click("detach").hover("click-me");
    assert_eq!(kit.color_at("attach-left"), BUTTON_BG);
}
on_every_runtime!(attach_buttons_show_target, |_, _| Testing::default());

// ---- GPUI components ----

fn toggle_switches_and_shows_state(kit: &mut impl Driver<Testing>) {
    kit.click("toggle").hover("click-me");
    assert!(kit.read(|h| h.toggled));
    assert_eq!(kit.color_at("toggle"), BUTTON_ACTIVE_BG);
    kit.click("toggle").hover("click-me");
    assert!(!kit.read(|h| h.toggled));
    assert_eq!(kit.color_at("toggle"), BUTTON_BG);
}
on_every_runtime!(toggle_switches_and_shows_state, |_, _| Testing::default());

/// `.active()`: the appearance changes only while pressed.
fn pressed_style_while_held(kit: &mut impl Driver<Testing>) {
    kit.down("hold-me");
    assert_eq!(kit.color_at("hold-me"), BUTTON_ACTIVE_BG);
    kit.up();
    assert_eq!(kit.color_at("hold-me"), BUTTON_HOVER_BG);
}
on_every_runtime!(pressed_style_while_held, |_, _| Testing::default());

/// `.on_drag()` / `.drag_over()` / `.on_drop()`: while carrying, the drop target lights up, and releasing delivers the drop.
fn drag_and_drop(kit: &mut impl Driver<Testing>) {
    kit.down("drag-chip").hover("drop-zone");
    assert_eq!(kit.color_at("drop-zone"), DROP_TARGET_BG);
    kit.up();
    assert_eq!(kit.read(|h| h.dropped), 1);
    assert_ne!(kit.color_at("drop-zone"), DROP_TARGET_BG);
}
on_every_runtime!(drag_and_drop, |_, _| Testing::default());

/// `.on_scroll_wheel()` and `.overflow_y_scroll()`: the scroll amount arrives and the list actually moves.
fn scroll_moves_list(kit: &mut impl Driver<Testing>) {
    kit.scroll("scroll-list", 1.0).scroll("scroll-list", -3.0);
    assert_eq!(kit.read(|h| h.scrolled), -2.0);
    assert!(
        kit.read(|h| h.list.offset().y) < gpui::px(0.0),
        "moving down"
    );
}
on_every_runtime!(scroll_moves_list, |_, _| Testing::default());

/// `deferred(anchored())`: opening the popup and choosing an item closes it. Pressing outside also closes it.
fn popup_menu_picks_item(kit: &mut impl Driver<Testing>) {
    kit.click("open-menu").click("menu-b");
    assert_eq!(
        kit.read(|h| (h.picked, h.menu_open)),
        (Some("menu-b"), false)
    );
    kit.click("open-menu");
    assert!(kit.read(|h| h.menu_open));
    kit.click("click-me");
    assert!(!kit.read(|h| h.menu_open));
}
on_every_runtime!(popup_menu_picks_item, |_, _| Testing::default());

/// `.track_focus()` / `.focus()`: focus moves to the pressed input field, visible in its appearance.
fn click_focuses_field(kit: &mut impl Driver<Testing>) {
    kit.click("text-field").hover("click-me");
    assert_eq!(kit.color_at("text-field"), FIELD_FOCUS_BG);
}
on_every_runtime!(click_focuses_field, |_, _| Testing::default());

/// `.on_key_down()`: only novr has a keyboard.
#[gpui::test]
fn typing_goes_to_focused_field(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.type_keys("x"); // nothing is entered while there is no focus
    kit.click("text-field").type_keys("h i space o k backspace");
    assert_eq!(kit.read(|h| h.text.clone()), "hi o");
}

/// This only checks UI logic, so novr is enough.
#[gpui::test]
fn hover_alone_does_not_click(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.hover("click-me");
    assert_eq!(kit.read(|h| h.clicks), 0);
}
