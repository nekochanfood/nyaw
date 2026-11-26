//! デジタル文字盤: 時刻を数字で、秒まで表示する。デザインごとに、数字の表示方式（液晶・フリップ・ターミナル）が異なる。

use design::{font, font_size, radius, spacing};
use gpui::{Div, FontWeight, div, prelude::*, px, relative, rgba};

use super::Face;

/// 液晶: 7 セグメントの数字。消灯セグメントも薄く表示される。下部のバーが秒で充填される（アクセントカラー）。
pub fn lcd(f: &Face) -> Div {
    let t = f.theme;
    // 消灯セグメント: テキスト色の極低不透明度。
    let unlit = t.neutral_text & !0xff | 0x1c;
    // `all` は全セグメント点灯文字（同一幅のため、正確に重なる）。
    let digits = |text: String, all: &'static str, size: f32| {
        div()
            .relative()
            .font(font::segment())
            .text_size(f.px(size))
            .line_height(f.px(size * 1.15))
            .child(div().text_color(rgba(unlit)).child(all))
            .child(div().absolute().top_0().left_0().child(text))
    };
    let bar = div()
        .w(f.px(180.0))
        .h(f.px(6.0))
        .rounded(px(radius::FULL))
        .bg(rgba(t.groove))
        .child(
            div()
                .w(relative(f.minute_progress()))
                .h_full()
                .rounded(px(radius::FULL))
                .bg(rgba(t.accent_solid)),
        );
    f.frame()
        .gap(f.px(spacing::SM))
        .child(
            f.date()
                .font(font::mono())
                .text_size(f.px(font_size::MD))
                .text_color(rgba(t.neutral_text_low)),
        )
        .child(digits(f.clock("%H:%M"), "88:88", 84.0).debug_selector(|| "time".into()))
        .child(digits(f.clock("%S"), "88", 34.0).text_color(rgba(t.neutral_text_low)))
        .child(bar)
}

/// フリップ時計: 時と分を 1 枚ずつのカードに表示する（カード中央に折れ目）。カード間のドットはアクセントカラー。
pub fn flip(f: &Face) -> Div {
    let t = f.theme;
    let (width, height) = (138.0, 156.0);
    let card = |text: String| {
        div()
            .relative()
            .w(f.px(width))
            .h(f.px(height))
            .rounded(f.px(radius::LG))
            .bg(rgba(t.control))
            .flex()
            .items_center()
            .justify_center()
            .text_size(f.px(104.0))
            .font_weight(FontWeight::BOLD)
            .child(text)
            // 折れ目: カードを上下に分割する、カード色の細い線。
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top(f.px(height / 2.0 - 1.5))
                    .w_full()
                    .h(f.px(3.0))
                    .bg(rgba(t.card)),
            )
    };
    let dot = || {
        div()
            .size(f.px(12.0))
            .rounded(px(radius::FULL))
            .circular_corners()
            .bg(rgba(t.accent_solid))
    };
    let colon = div()
        .flex()
        .flex_col()
        .gap(f.px(24.0))
        .child(dot())
        .child(dot());
    f.frame()
        .gap(f.px(spacing::MD))
        .child(
            f.date()
                .text_size(f.px(font_size::MD))
                .text_color(rgba(t.neutral_text_low)),
        )
        .child(
            div()
                .debug_selector(|| "time".into())
                .flex()
                .items_center()
                .gap(f.px(spacing::SM))
                .child(card(f.clock("%H")))
                .child(colon)
                .child(card(f.clock("%M"))),
        )
        .child(
            div()
                .px(f.px(spacing::MD))
                .rounded(f.px(radius::MD))
                .bg(rgba(t.control))
                .text_size(f.px(font_size::LG))
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(t.neutral_text_low))
                .child(f.clock("%S")),
        )
}

/// ターミナル: 等幅フォントで、コマンド出力のように日付と時刻を表示する。カーソル（アクセントカラー）は 1 秒ごとに点滅する。
pub fn terminal(f: &Face) -> Div {
    let t = f.theme;
    let prompt = |command: &'static str| {
        div()
            .text_color(rgba(t.neutral_text_low))
            .child(format!("$ {command}"))
    };
    let lit = f.now.timestamp() % 2 == 0;
    let cursor = div()
        .w(f.px(24.0))
        .h(f.px(44.0))
        .bg(rgba(if lit { t.accent_solid } else { 0 }));
    let lines = div()
        .font(font::mono())
        .text_size(f.px(22.0))
        .line_height(f.px(30.0))
        .flex()
        .flex_col()
        .child(prompt("date"))
        .child(f.date())
        .child(prompt("time").mt(f.px(spacing::SM)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(f.px(6.0))
                .text_size(f.px(44.0))
                .line_height(f.px(54.0))
                .child(
                    div()
                        .debug_selector(|| "time".into())
                        .child(f.clock("%H:%M:%S")),
                )
                .child(cursor),
        );
    f.frame().child(lines)
}
