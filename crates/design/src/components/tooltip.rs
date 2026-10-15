//! ツールチップ。ホバー中の要素が [`Tooltips`] に文字を置き、カーソル側（`beam`）が [`tooltip`] の箱で描く。
//! 箱は黒の半透明に白い文字のカプセル形で、テーマによらない。

use std::collections::HashMap;

use gpui::{App, Div, SharedString, Window, WindowId, div, prelude::*, px, rgba};

use crate::{Theme, font_size, radius, spacing};

/// 箱の不透明度（0..=255）。透けすぎると後ろの文字と混ざるので、白い面の上でも文字が 7:1 を保つ濃さにする。
pub const TOOLTIP_ALPHA: u32 = 0xcc;
/// 文字の行の高さ。
const LINE: f32 = font_size::MD * 1.4;
/// 縦の余白と、それより広い横の余白（カプセルの丸い端に文字がかからないよう、高さの半分以上）。
const PAD_Y: f32 = spacing::SM;
const PAD_X: f32 = spacing::XL;
/// 箱の高さ。
pub const TOOLTIP_HEIGHT: f32 = LINE + PAD_Y * 2.0;

/// ツールチップの要求（GPUI のグローバル）。ウィンドウごとに、ホバーしている要素（`id`）とその文字。
/// 外れたら消す。どの手のカーソルに出すかは UI 層が決める。
#[derive(Default)]
pub struct Tooltips(pub HashMap<WindowId, (&'static str, SharedString)>);

impl gpui::Global for Tooltips {}

/// 要素 `id` の `on_hover` から呼ぶ。入ったら `text` を置き、出たら（自分が置いたものなら）消す。
/// 隣の要素へ移ると、入る・出るの順が前後しても後から入った方が残る。
pub fn hover_tooltip(
    id: &'static str,
    text: SharedString,
    hovered: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let w = window.window_handle().window_id();
    let map = &mut cx.default_global::<Tooltips>().0;
    if hovered {
        map.insert(w, (id, text));
    } else if map.get(&w).is_some_and(|(i, _)| *i == id) {
        map.remove(&w);
    }
}

/// 文字 `text` を 1 行で置いたときの箱の幅（px）。
pub fn tooltip_width(text: &SharedString, window: &mut Window) -> f32 {
    let run = gpui::TextRun {
        len: text.len(),
        font: crate::font::font(),
        color: rgba(0xffffffff).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.clone(), px(font_size::MD), &[run], None);
    f32::from(line.width).ceil() + PAD_X * 2.0
}

/// 幅 `width`（[`tooltip_width`]。動かしている間は途中の値）の箱。収まらない文字は省略せず箱で切る
/// （幅が伸び縮みする間に省略記号が出ないように。中央から開いていくように見える）。
// ponytail: 1 行だけ。パネルの幅（640px）を超える文字は切れる。長い説明が要るなら折り返しを入れる。
pub fn tooltip(text: SharedString, width: f32) -> Div {
    let theme = Theme::DARK;
    div()
        .w(px(width))
        .h(px(TOOLTIP_HEIGHT))
        .px(px(PAD_X))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded(px(radius::FULL))
        .bg(rgba(theme.tooltip & !0xff | TOOLTIP_ALPHA))
        .text_color(rgba(theme.on_tooltip))
        .font(crate::font::font())
        .text_size(px(font_size::MD))
        .line_height(px(LINE))
        .whitespace_nowrap()
        .child(div().flex_none().child(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sRGB の 0..=255 の相対輝度。
    fn luminance(c: [f32; 3]) -> f32 {
        let l = |v: f32| {
            let v = v / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
    }

    #[test]
    fn text_stays_readable_over_a_white_backdrop() {
        // いちばん明るい背景（白）に半透明の箱を重ねても、文字は本文の 7:1。
        let a = TOOLTIP_ALPHA as f32 / 255.0;
        let rgb = |c: u32| [(c >> 24) as f32, (c >> 16 & 0xff) as f32, (c >> 8 & 0xff) as f32];
        let bg = rgb(Theme::DARK.tooltip).map(|v| v * a + 255.0 * (1.0 - a));
        let fg = luminance(rgb(Theme::DARK.on_tooltip));
        let ratio = (fg + 0.05) / (luminance(bg) + 0.05);
        assert!(ratio >= 7.0, "{ratio}");
        assert_eq!(Theme::DARK.tooltip, Theme::LIGHT.tooltip);
    }

    #[test]
    fn capsule_ends_clear_the_text() {
        assert!(PAD_X >= TOOLTIP_HEIGHT / 2.0 && PAD_X > PAD_Y);
    }

    #[gpui::test]
    fn the_last_entered_element_owns_the_tooltip(cx: &mut gpui::TestAppContext) {
        let w = cx.add_empty_window();
        w.update(|window, cx| {
            hover_tooltip("a", "A".into(), true, window, cx);
            // 隣へ移ると、入るのが出るより先に届くことがある。
            hover_tooltip("b", "B".into(), true, window, cx);
            hover_tooltip("a", "A".into(), false, window, cx);
            let id = window.window_handle().window_id();
            assert_eq!(cx.global::<Tooltips>().0[&id].1, "B");
            hover_tooltip("b", "B".into(), false, window, cx);
            assert!(cx.global::<Tooltips>().0.is_empty());
        });
    }

    #[gpui::test]
    fn longer_text_makes_a_wider_box(cx: &mut gpui::TestAppContext) {
        let w = cx.add_empty_window();
        w.update(|window, _| {
            let short = tooltip_width(&"OK".into(), window);
            let long = tooltip_width(&"A much longer tooltip".into(), window);
            assert!(short >= PAD_X * 2.0 && long > short, "{short} {long}");
        });
    }
}
