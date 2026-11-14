//! フォントの見本（[`TestingFont`]）。

use super::*;

/// フォントの見本（`testing-font`）: 英数・かな漢字・混在を、太さと大きさを変えて並べる。
pub struct TestingFont {}

impl TestingFont {
    pub fn new(_panel: PanelId) -> Self {
        Self {}
    }
}

impl Render for TestingFont {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        use gpui::FontWeight;
        let sample = |id: &'static str, size: f32, weight: FontWeight, text: &'static str| {
            div()
                .debug_selector(|| id.into())
                .text_size(px(size))
                .font_weight(weight)
                .child(text)
        };
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(sample(
                "font-latin",
                24.0,
                FontWeight::NORMAL,
                "Gen Interface JP: The quick brown fox jumps over the lazy dog 0123456789",
            ))
            .child(sample(
                "font-latin-bold",
                24.0,
                FontWeight::BOLD,
                "Gen Interface JP Bold: Sphinx of black quartz, judge my vow",
            ))
            .child(sample(
                "font-ja",
                24.0,
                FontWeight::NORMAL,
                "Gen Interface JP 和文: 吾輩は猫である。名前はまだ無い。",
            ))
            .child(sample(
                "font-ja-bold",
                24.0,
                FontWeight::BOLD,
                "Gen Interface JP 太字: いろはにほへと ちりぬるを",
            ))
            .child(sample(
                "font-mixed",
                19.0,
                FontWeight::NORMAL,
                "Mixed 混在: Hello、世界! VR パネル 123 ABC あいう",
            ))
            .child(sample(
                "font-small",
                14.0,
                FontWeight::NORMAL,
                "Small 14px: 小さい文字の読みやすさ — ABC abc 漢字かな",
            ))
    }
}
