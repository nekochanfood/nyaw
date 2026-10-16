use gpui::TestAppContext;
use testing::{EmojiTest, Testing};
use testkit::{Driver, NoVrKit, on_every_runtime};

fn click_emoji(kit: &mut impl Driver<EmojiTest>) {
    kit.click("emoji-heart").click("emoji-heart");
    assert_eq!(kit.read(|view| view.clicks), 2);
}
on_every_runtime!(click_emoji, |_, _| EmojiTest::default());

#[gpui::test]
fn testing_opens_emoji_screen(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.click("emoji-test");
    assert!(!kit.kernel.panel(1).closed);
    // Closed from its navigation bar (opened right after it).
    kit.select_panel(2).click("nav-close");
    assert!(kit.kernel.panel(1).closed);
}
