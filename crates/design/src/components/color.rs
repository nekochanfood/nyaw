//! 色を選ぶ部品: 色の見本（[`swatch`]）と、パーソナルカラーの選択（[`accent_picker`]）。

use std::rc::Rc;

use gpui::{App, Div, SharedString, Window, div, prelude::*, px, rgba};

use super::{Button, Variant, round_button, slider};
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
    round_button(theme, id, name, Variant::Ghost).child(
        div()
            .flex_none()
            .p(px(SWATCH_RING))
            .border(px(SWATCH_RING))
            .border_color(rgba(ring))
            .rounded(px(radius::FULL))
            .child(
                div()
                    .size(px(SWATCH))
                    .rounded(px(radius::FULL))
                    .bg(rgba(color)),
            ),
    )
}

/// パーソナルカラーの選択: 用意した色の見本（`accent-<名前>`）と、色相・鮮やかさ・明るさのスライダー
/// （`accent-hue` / `accent-chroma` / `accent-lightness`）。選ぶ・動かすたびに `on_pick` が新しい色で呼ばれる。
/// `label` は名前（`blue` などの色の名前と `hue` / `chroma` / `lightness`）から文言を引く。`width` はスライダーの長さ（px）。
pub fn accent_picker(
    theme: &Theme,
    accent: Accent,
    width: f32,
    label: impl Fn(&str) -> SharedString,
    on_pick: impl Fn(Accent, &mut Window, &mut App) + 'static,
) -> Div {
    let on_pick = Rc::new(on_pick);
    let swatches = Accent::PRESETS.map(|(name, preset)| {
        let on_pick = on_pick.clone();
        swatch(
            theme,
            format!("accent-{name}"),
            preset.color(),
            label(name),
            preset == accent,
        )
        .on_click(move |_, window, cx| on_pick(preset, window, cx))
    });
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
        .child(div().flex().flex_wrap().children(swatches))
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
