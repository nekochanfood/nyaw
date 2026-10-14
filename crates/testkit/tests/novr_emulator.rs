//! novr のエミュレーター固有の操作シナリオ。パネルはエミュレーターの画面に映り、マウスは右手のコントローラ:
//! 左クリック = トリガー、右クリック = グリップ、矢印キー = 右手のスティック、WASD = 左手のスティック（片手操作なので左手は何も指さない）。
//! デバイスは実在しないので、取り付けたパネルは追従しない。

use gpui::TestAppContext;
use testing::{BUTTON_BG, BUTTON_HOVER_BG, Testing};
use testkit::{Button, Device, Driver, Hand, NoVrKit, Vec2};
use vr::VrBackend;

fn is_gray(c: u32) -> bool {
    let [_, r, g, b] = c.to_be_bytes();
    r == g && g == b
}

#[gpui::test]
fn panel_is_shown_over_gray_checker(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    let shot = kit.screenshot();
    assert_eq!((shot.width, shot.height), (1280, 720));
    let (a, b) = (
        kit.color_on_screen(Vec2::new(0.0, 0.0)),
        kit.color_on_screen(Vec2::new(16.0, 0.0)),
    );
    assert!(is_gray(a) && is_gray(b) && a != b, "無彩色の市松模様");
    assert_eq!(kit.color_at("click-me"), BUTTON_BG);
}

#[gpui::test]
fn mouse_hovers_and_leaving_the_screen_drops_the_hand(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.hover("click-me");
    assert_eq!(kit.color_at("click-me"), BUTTON_HOVER_BG);
    // 画面の外では右手が無いので、押してもクリックにならない。
    kit.move_mouse(None)
        .press(Button::Trigger)
        .release(Button::Trigger);
    assert_eq!(kit.read(|h| h.clicks), 0);
    assert!(
        kit.kernel
            .vr
            .poll()
            .controllers
            .iter()
            .all(|c| c.hand == Hand::Left)
    );
}

#[gpui::test]
fn right_click_grabs_and_carries_panel(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    let before = kit.kernel.panel(0).pose;
    let at = kit.on_screen("click-me");
    kit.move_mouse(Some(at)).press(Button::Grab);
    assert!(kit.kernel.status(0).grabbed);
    // 掴み移動はなめらかに追いつくので、少しずつ動かしてから待つ。
    for i in 1..=10 {
        kit.move_mouse(Some(at + Vec2::new(20.0 * i as f32, 0.0)));
    }
    for _ in 0..10 {
        kit.step();
    }
    kit.release(Button::Grab);
    let after = kit.kernel.panel(0).pose;
    assert!(after.position.x > before.position.x + 0.1, "{after:?}");
    assert!(!kit.kernel.status(0).grabbed);
    assert_eq!(kit.read(|h| h.clicks), 0, "掴んでもクリックにならない");
    // 運んだ先でもクリックできる。
    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}

#[gpui::test]
fn arrows_scroll_but_wasd_does_not_in_one_handed_mode(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.hover("scroll-list")
        .stick(Hand::Left, Vec2::new(0.0, -1.0))
        .stick(Hand::Left, Vec2::ZERO);
    assert_eq!(kit.read(|h| h.scrolled), 0.0, "左手は何も指していない");
    kit.stick(Hand::Right, Vec2::new(0.0, -1.0))
        .stick(Hand::Right, Vec2::ZERO);
    assert_ne!(kit.read(|h| h.scrolled), 0.0);
}

#[gpui::test]
fn attached_panel_does_not_follow_emulated_hand(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    let before = kit.kernel.panel(0).pose;
    kit.click("attach-left");
    assert_eq!(
        kit.kernel.status(0).attached,
        Some(Device::Controller(Hand::Left))
    );
    kit.step();
    assert_eq!(kit.kernel.panel(0).pose, before);
    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
