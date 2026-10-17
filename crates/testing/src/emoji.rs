use super::*;

/// Twemoji のテキスト・アイコン表示を確認する画面。
#[derive(Default)]
pub struct EmojiTest {
    pub clicks: usize,
    scroll: ScrollHandle,
}

impl EmojiTest {
    pub fn new(_panel: PanelId) -> Self {
        Self { ..Self::default() }
    }
}

impl Render for EmojiTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let samples = div()
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .child(div().child(i18n::t(cx, "emoji.inline")))
            .children([font_size::SM, font_size::MD, font_size::LG].map(|edge| {
                div().text_size(px(edge)).child(design::emoji_text(
                    "Hello 😀! こんにちは 🐈❤️ — 👍🏽 👩‍💻 🇯🇵 1️⃣ 🏳️‍🌈",
                ))
            }))
            .child(
                div()
                    .w(px(260.))
                    .debug_selector(|| "emoji-wrapped".into())
                    .child(design::emoji_text(i18n::t(cx, "emoji.wrap"))),
            )
            .child(design::emoji_text(i18n::t(cx, "emoji.fallback")))
            .child(
                div()
                    .text_size(px(font_size::SM))
                    .child("Twemoji v17.0.3 · Twitter, Inc. and contributors · CC BY 4.0"),
            );
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::LG))
            .child(div().child(i18n::t(cx, "emoji.icons")))
            .child(
                row()
                    .children([24., 40., 64.].map(|edge| design::emoji("🐈").size(px(edge))))
                    .child(
                        components::emoji_button(
                            &theme,
                            "emoji-heart",
                            "❤️",
                            i18n::t(cx, "emoji.favorite"),
                            Variant::Default,
                        )
                        .debug_selector(|| "emoji-heart".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.clicks += 1;
                            cx.notify();
                        })),
                    )
                    .child(i18n::tf(cx, "testing.clicked", &[("count", &self.clicks)])),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        components::free_scroll(
                            div()
                                .id("emoji-samples")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll),
                        )
                        .child(samples),
                    )
                    .child(components::scrollbars(&theme, "emoji-bars", &self.scroll)),
            )
    }
}
