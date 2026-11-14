//! 透過したパネル（[`TestingTransparent`]）。

use super::*;

/// 半透明の背景に描画される画面（`testing-transparent`）。右上の Close ボタンで自分を閉じる。
pub const TRANSPARENT_BG: u32 = 0x202020a0;

#[derive(Default)]
pub struct TestingTransparent {}

impl TestingTransparent {
    pub fn new(_panel: PanelId) -> Self {
        Self {}
    }
}

impl Render for TestingTransparent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_3()
            .bg(rgba(TRANSPARENT_BG))
            .text_color(rgba(theme.neutral_text))
            .child(i18n::t(cx, "testing.open.transparent"))
    }
}
