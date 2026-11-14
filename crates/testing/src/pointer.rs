//! ポインタの方式を試す画面（[`PointerTest`]）。

use super::*;

/// ポインタの方式（マウス / タッチのエミュレート）を試す画面（`pointer-test`）。
/// 方式は全体の設定に従う（画面からは切り替えない）。同じ操作がどう効くかを見る:
/// マウスでは `drag-chip` を `drop-zone` へ運べ、タッチでは `swipe-list` を押したまま動かすとスクロールする。
/// `tap-me` はどちらでも押せる。
#[derive(Default)]
pub struct PointerTest {
    pub taps: usize,
    pub dropped: usize,
    /// スワイプする一覧の状態（`offset().y` が負 = 下へスクロール済み）。
    pub list: ScrollHandle,
}

impl PointerTest {
    pub fn new(_panel: PanelId) -> Self {
        Self::default()
    }
}

impl Render for PointerTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let swipe_rows: Vec<SharedString> = (0..LIST_ROWS)
            .map(|i| i18n::tf(cx, "pointer.swipe_row", &[("index", &i)]))
            .collect();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgba(theme.card))
            .text_color(rgba(theme.neutral_text))
            .child(
                row().child(
                    button(
                        &theme,
                        "tap-me",
                        i18n::tf(cx, "pointer.tapped", &[("count", &self.taps)]),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.taps += 1;
                        cx.notify();
                    })),
                ),
            )
            .child(
                row()
                    .child(
                        button(&theme, "drag-chip", i18n::t(cx, "testing.drag_me"), false)
                            .on_drag(Chip, |_, _, _, cx| ui::drag_preview(cx, ChipPreview)),
                    )
                    .child(
                        div()
                            .id("drop-zone")
                            .debug_selector(|| "drop-zone".into())
                            .p_2()
                            .bg(rgba(theme.control))
                            .drag_over::<Chip>(|s, _, _, _| s.bg(rgb(DROP_TARGET_BG)))
                            .on_drop(cx.listener(|this, _: &Chip, _, cx| {
                                this.dropped += 1;
                                cx.notify();
                            }))
                            .child(i18n::tf(cx, "pointer.dropped", &[("count", &self.dropped)])),
                    ),
            )
            .child(
                div()
                    .id("swipe-list")
                    .debug_selector(|| "swipe-list".into())
                    .h(gpui::px(240.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.list)
                    .bg(rgba(theme.control))
                    .children(
                        swipe_rows
                            .into_iter()
                            .map(|row| div().h(design::font::scaled(ROW_PX)).px_2().child(row)),
                    ),
            )
    }
}
