//! 色を選ぶ部品: 色の見本（[`swatch`]）、パーソナルカラーの履歴の行（[`accent_row`]）、自分で色を作るスライダー（[`accent_sliders`]）。

use std::rc::Rc;

use gpui::{App, ClickEvent, Div, SharedString, Window, div, prelude::*, px, rgba};

use super::{Button, Variant, icon_button, round_button, slider};
use crate::{Accent, Theme, font_size, radius, spacing};

/// 色の見本（[`swatch`]）の丸の直径と、選んでいる見本を囲む輪の太さ（丸との間も同じだけ空ける）。
const SWATCH: f32 = 28.0;
const SWATCH_RING: f32 = 3.0;
/// スライダーの上に敷く色の帯の高さと、刻む数。
const STRIP: f32 = 8.0;
const STRIP_CELLS: usize = 36;
/// スライダーのハンドルの半径（帯の両端をハンドルの動く範囲に合わせる）。
const HANDLE_RADIUS: f32 = 10.0;

/// 色の見本のボタン: 丸いボタン（[`round_button`]）の中に `color` の丸。`selected` なら丸を輪で囲む。
/// 色だけでは伝わらないので、色の名前 `name` を説明（ツールチップ）に出す。
pub fn swatch(
    theme: &Theme,
    id: impl Into<SharedString>,
    color: u32,
    name: impl Into<SharedString>,
    selected: bool,
) -> Button {
    let ring = if selected { theme.neutral_text } else { 0 };
    // 見本は正円（全域のスーパーエリプスからは外す）。
    round_button(theme, id, name, Variant::Ghost)
        .circular_corners()
        .child(
            div()
                .flex_none()
                .p(px(SWATCH_RING))
                .border(px(SWATCH_RING))
                .border_color(rgba(ring))
                .rounded(px(radius::FULL))
                .circular_corners()
                .child(
                    div()
                        .size(px(SWATCH))
                        .rounded(px(radius::FULL))
                        .circular_corners()
                        .bg(rgba(color)),
                ),
        )
}

/// パーソナルカラーの履歴の行: `colors`（[`Accent::history`]）の見本（`accent-<名前>`。作った色は `accent-<RRGGBB>`）を左右中央に
/// 並べ、右端に自分で色を作るボタン（`accent-custom`）。見本を押すと `on_pick`、プラスを押すと `on_custom`（色を作る画面を開くのは
/// 呼び出し側）。`label` は名前（色の名前と `custom`）から文言を引く。名前の無い色は `#RRGGBB` を説明に出す。
pub fn accent_row(
    theme: &Theme,
    accent: Accent,
    colors: &[Accent],
    label: impl Fn(&str) -> SharedString,
    on_pick: impl Fn(Accent, &mut Window, &mut App) + 'static,
    on_custom: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let on_pick = Rc::new(on_pick);
    let swatches = colors.iter().map(|&color| {
        let on_pick = on_pick.clone();
        let hex = color.hex();
        let (id, name) = match color.name() {
            Some(name) => (format!("accent-{name}"), label(name)),
            None => (format!("accent-{}", &hex[1..]), hex.into()),
        };
        swatch(theme, id, color.color(), name, color == accent)
            .on_click(move |_, window, cx| on_pick(color, window, cx))
    });
    let custom = icon_button(
        theme,
        "accent-custom",
        &crate::icons::PLUS,
        label("custom"),
        Variant::Default,
    )
    .circular_corners()
    .on_click(on_custom);
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_center()
        .children(swatches)
        // 見本の丸（ボタンより小さい）と同じだけ、プラスの丸も見本から離す。
        .child(div().pl(px(spacing::SM)).child(custom))
}

/// 自分で色を作るスライダー: 色相・鮮やかさ・明るさ（`accent-hue` / `accent-chroma` / `accent-lightness`）。動かすたびに
/// `on_pick` が新しい色で呼ばれる。上の帯は動かすとどう変わるか。`label` は名前（`hue` / `chroma` / `lightness`）から
/// 文言を引く。`width` はスライダーの長さ（px）。
pub fn accent_sliders(
    theme: &Theme,
    accent: Accent,
    width: f32,
    label: impl Fn(&str) -> SharedString,
    on_pick: impl Fn(Accent, &mut Window, &mut App) + 'static,
) -> Div {
    let on_pick = Rc::new(on_pick);
    // スライダー 1 本: 名前、動かすとどう変わるかを見せる色の帯、スライダー。`at` は位置（0〜1）からその色。
    let row = |id: &'static str, name: &str, value: f32, at: fn(Accent, f32) -> Accent| {
        let on_pick = on_pick.clone();
        let cells = (0..STRIP_CELLS).map(|i| {
            let t = (i as f32 + 0.5) / STRIP_CELLS as f32;
            div().flex_1().h_full().bg(rgba(at(accent, t).color()))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(spacing::XS))
            .child(
                div()
                    .text_size(px(font_size::SM))
                    .text_color(rgba(theme.neutral_text_low))
                    .child(label(name)),
            )
            .child(
                div()
                    .mx(px(HANDLE_RADIUS))
                    .w(px(width - 2.0 * HANDLE_RADIUS))
                    .h(px(STRIP))
                    .rounded(px(radius::FULL))
                    .overflow_hidden()
                    .flex()
                    .children(cells),
            )
            .child(
                slider(theme, id, value, move |v, window, cx| {
                    on_pick(at(accent, v), window, cx)
                })
                .length(width),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::MD))
        .child(row("accent-hue", "hue", accent.hue / HUE_MAX, |a, t| {
            Accent::new(a.lightness, a.chroma, t * HUE_MAX)
        }))
        .child(row(
            "accent-chroma",
            "chroma",
            accent.chroma / Accent::MAX_CHROMA,
            |a, t| Accent::new(a.lightness, t * Accent::MAX_CHROMA, a.hue),
        ))
        .child(row(
            "accent-lightness",
            "lightness",
            accent.lightness,
            |a, t| Accent::new(t, a.chroma, a.hue),
        ))
}

/// 色相のスライダーの右端（度）。360 は 0 に巻き戻ってハンドルが左端へ飛ぶので、手前で止める。
const HUE_MAX: f32 = 359.0;
