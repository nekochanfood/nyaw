use super::*;
use design::{Logo, Mode};

/// ロゴの読み込みと、大きさごとの見え方を確かめる画面。
/// 行は 16〜256 px（アイコンに埋め込むサイズ）、列はライト用・ダーク用（それぞれの地色の上に置く）。
#[derive(Default)]
pub struct LogoTest {
    scroll: ScrollHandle,
}

impl LogoTest {
    pub fn new(_panel: PanelId) -> Self {
        Self::default()
    }
}

/// 同じ大きさのロゴをライト用・ダーク用の地の上に並べた行。
fn size_row(edge: f32) -> Div {
    let tile = |mode: Mode| {
        let bg = Theme::of(mode);
        div()
            .flex()
            .items_center()
            .justify_center()
            .w(px(edge + 16.))
            .h(px(edge + 16.))
            .rounded(px(radius::MD))
            .bg(rgba(bg.card))
            .child(design::logo(Logo::Mini).height(px(edge)).mode(mode))
    };
    row()
        .gap(px(spacing::MD))
        .child(div().w(px(48.)).child(format!("{edge} px")))
        .child(tile(Mode::Light))
        .child(tile(Mode::Dark))
}

impl Render for LogoTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let samples = div()
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .child(div().child(i18n::t(cx, "logo.current")))
            .child(
                row()
                    .gap(px(spacing::LG))
                    .child(design::logo(Logo::Full).height(px(64.)))
                    .child(design::logo(Logo::Mini).height(px(64.))),
            )
            .child(div().child(i18n::t(cx, "logo.sizes")))
            .children([16., 24., 32., 48., 64., 128., 256.].map(size_row));
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::LG))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        components::free_scroll(
                            div()
                                .id("logo-samples")
                                .debug_selector(|| "logo-samples".into())
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll),
                        )
                        .child(samples),
                    )
                    .child(components::scrollbars(&theme, "logo-bars", &self.scroll)),
            )
    }
}
