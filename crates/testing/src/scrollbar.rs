//! 縦・横・縦横のスクロールバーを検証する画面（[`TestingScrollbar`]）。

use super::*;

/// スクロールバーのテスト画面（`testing-scrollbar`）: 縦方向・横方向・縦横方向の 3 つのオーバーフロー領域。
/// 領域は `scrollbar-v` / `scrollbar-h` / `scrollbar-both`、バーは `{領域}-bars-v` / `-h`（`-thumb` 付きがサム）。
pub struct TestingScrollbar {
    pub vertical: ScrollHandle,
    pub horizontal: ScrollHandle,
    pub both: ScrollHandle,
    /// 縦方向領域内のネストされたスクロール要素。
    pub inner: ScrollHandle,
}

impl TestingScrollbar {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            vertical: ScrollHandle::new(),
            horizontal: ScrollHandle::new(),
            both: ScrollHandle::new(),
            inner: ScrollHandle::new(),
        }
    }
}

/// コンテンツの 1 セル（色相で位置を識別可能）。
pub(crate) fn cell(i: usize) -> Div {
    div()
        .flex_none()
        .size(px(48.0))
        .rounded(px(8.0))
        .bg(hsla((i % 12) as f32 / 12.0, 0.5, 0.45, 1.0))
        .flex()
        .items_center()
        .justify_center()
        .child(format!("{i}"))
}

impl Render for TestingScrollbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        // 領域の枠: コンテナ（relative）にスクロール本体とバーを重ねる。
        let area =
            |id: &'static str, pull: &'static str, handle: &ScrollHandle, body: Stateful<Div>| {
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .rounded(px(8.0))
                    .bg(rgba(theme.control))
                    .child(body.size_full().track_scroll(handle))
                    .child(components::scrollbars(&theme, id, handle).pulled_by(pull))
            };
        let vertical = area(
            "bars-v",
            "pull-v",
            &self.vertical,
            div()
                .id("scrollbar-v")
                .debug_selector(|| "scrollbar-v".into())
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(
                    components::elastic("pull-v", &self.vertical)
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .child(
                            // ネストスクロール: ポインタが接触している内側の要素のみスクロールする。
                            div()
                                .debug_selector(|| "scrollbar-nested".into())
                                .relative()
                                .flex_none()
                                .h(px(120.0))
                                .rounded(px(8.0))
                                .bg(rgba(theme.card))
                                .child(
                                    components::claim_scroll(
                                        div()
                                            .id("scrollbar-inner")
                                            .debug_selector(|| "scrollbar-inner".into())
                                            .size_full()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.inner),
                                        &self.inner,
                                    )
                                    .child(
                                        components::elastic("pull-inner", &self.inner)
                                            .flex_none()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .p_1()
                                            .children((0..20).map(|i| {
                                                div().flex_none().child(format!("inner {i}"))
                                            })),
                                    ),
                                )
                                .child(
                                    components::scrollbars(&theme, "bars-inner", &self.inner)
                                        .pulled_by("pull-inner"),
                                ),
                        )
                        .children((0..40).map(|i| cell(i).w_full())),
                ),
        );
        let horizontal = area(
            "bars-h",
            "pull-h",
            &self.horizontal,
            div()
                .id("scrollbar-h")
                .debug_selector(|| "scrollbar-h".into())
                .overflow_x_scroll()
                .flex()
                .child(
                    components::elastic("pull-h", &self.horizontal)
                        .flex_none()
                        .flex()
                        .gap_2()
                        .p_2()
                        .children((0..40).map(cell)),
                ),
        );
        let both = area(
            "bars-both",
            "pull-both",
            &self.both,
            components::free_scroll(div().id("scrollbar-both"))
                .debug_selector(|| "scrollbar-both".into())
                .overflow_scroll()
                .flex()
                .flex_col()
                .child(
                    components::elastic("pull-both", &self.both)
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        // 幅をコンテンツに合わせないと、親の幅に制約されて水平オーバーフローが発生しない。
                        .w(px(30.0 * 56.0 + 16.0))
                        .children((0..20).map(|r| {
                            div()
                                .flex()
                                .flex_none()
                                .gap_2()
                                .children((0..30).map(move |c| cell(r * 30 + c)))
                        })),
                ),
        );
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap_2()
                    .child(vertical)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(horizontal)
                            .child(both),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
