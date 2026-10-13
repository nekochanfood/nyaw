//! Home パネルの基本操作。基本操作はどのランタイムでもネイティブに行えるべきなので、
//! シナリオは1回だけ書いて全ランタイム（novr / VR）で実行する。

use gpui::TestAppContext;
use testkit::{Driver, NoVrKit, on_every_runtime};
use ui::{BUTTON_BG, BUTTON_HOVER_BG, Home};

fn click_button(kit: &mut impl Driver<Home>) {
    kit.click("click-me").click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 2);
}
on_every_runtime!(click_button, |_, _| Home::default());

fn hover_highlights_button(kit: &mut impl Driver<Home>) {
    assert_eq!(kit.color_at("click-me"), BUTTON_BG);
    kit.hover("click-me");
    assert_eq!(kit.color_at("click-me"), BUTTON_HOVER_BG);
}
on_every_runtime!(hover_highlights_button, |_, _| Home::default());

/// UI のロジックだけの確認なので novr で十分。
#[gpui::test]
fn hover_alone_does_not_click(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Home::default());
    kit.hover("click-me");
    assert_eq!(kit.read(|h| h.clicks), 0);
}
