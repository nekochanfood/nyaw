//! 角を掴んで大きさを変えるパネル（[`ResizeTest`]）。

use super::*;

/// 角で変えられる解像度の範囲（`resize-test`・`resize-test-aspect` が開くパネル）。
pub const RESIZE_MIN: (u32, u32) = (400, 300);
pub const RESIZE_MAX: (u32, u32) = (1600, 1200);
/// 表示領域の幅に合わせて折り返すタイルの数。
const TILES: usize = 24;

/// 角を掴んで大きさを変えられるパネル。今の表示領域の大きさ（`resize-size`）と、幅に合わせて折り返すタイル（`resize-tile-N`）を表示する。
/// パネルの解像度がそのまま表示領域になるので、広げると並ぶタイルが増え、狭めると折り返す。
pub struct ResizeTest;

impl ResizeTest {
    /// パネル `from` の隣に開く起動仕様。`keep_aspect` なら縦横比を保って大きさを変える。
    pub fn spec(from: PanelId, keep_aspect: bool) -> PanelSpec {
        let title = if keep_aspect {
            "testing.open.resize_aspect"
        } else {
            "testing.open.resize"
        };
        let spec = PanelSpec::spawn(from, |_, _, _| ResizeTest)
            .corner_resize(RESIZE_MIN, RESIZE_MAX)
            .titled(title);
        if keep_aspect {
            spec.keep_aspect()
        } else {
            spec
        }
    }
}

impl Render for ResizeTest {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let viewport = window.viewport_size();
        let (w, h) = (
            f32::from(viewport.width).round(),
            f32::from(viewport.height).round(),
        );
        let range = |(w, h): (u32, u32)| format!("{w} × {h}");
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::LG))
            .child(
                div()
                    .debug_selector(|| "resize-size".into())
                    .text_size(px(font_size::LG))
                    .font_weight(FontWeight::BOLD)
                    .child(format!("{w} × {h} px")),
            )
            .child(
                div()
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::tf(
                        cx,
                        "testing.resize.hint",
                        &[("min", &range(RESIZE_MIN)), ("max", &range(RESIZE_MAX))],
                    )),
            )
            .child(row().children((0..TILES).map(|i| {
                div()
                    .debug_selector(move || format!("resize-tile-{i}"))
                    .size(px(72.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::LG))
                    .bg(rgba(theme.control))
                    .child(format!("{}", i + 1))
            })))
    }
}
