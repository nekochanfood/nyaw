//! ツールチップ。ホバー中の要素が [`Tooltips`] に文字を置き、カーソル側（`beam`）が [`tooltip`] の箱で描く。
//! 箱は黒の半透明に白い文字のカプセル形で、テーマによらない。

use std::collections::{HashMap, HashSet};

use gpui::{AnyWeakEntity, App, Div, SharedString, Window, WindowId, div, prelude::*, px, rgba};

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
/// 箱の高さ（m）の頭から 1m 先での見た目。離れるほど大きくして、この見た目を保つ（カーソルの丸 2 個分。`beam` が検査）。
/// ナビゲーションバーなど、頭から見た大きさを保つものはこれを物差しにする。
pub const TOOLTIP_HEIGHT_M: f32 = 0.024;

/// 入力を配送しているポインタ（0 = 左手、1 = 右手）と、配送先のウィンドウ。
#[derive(Default)]
pub struct TooltipSource(pub Option<u8>, pub HashSet<WindowId>);
impl gpui::Global for TooltipSource {}

/// ツールチップの要求（GPUI のグローバル）。ウィンドウと手ごとに、ホバーしている要素（`id`）とその文字、持ち主。
/// 外れたら消す。どの手のカーソルに出すかは UI 層が決める。
#[derive(Default)]
pub struct Tooltips(pub HashMap<(WindowId, u8), (SharedString, SharedString, AnyWeakEntity)>);

impl gpui::Global for Tooltips {}

impl Tooltips {
    /// 持ち主が描かれなくなった要求を消す。ホバー中の要素が消えると（押して画面が変わったなど）GPUI は外れたことを
    /// 知らせないので、要素の状態（持ち主）が捨てられたかで見る。UI 層が毎フレーム呼ぶ。
    pub fn drop_stale(&mut self) {
        self.0.retain(|_, (_, _, owner)| owner.upgrade().is_some());
    }
}

/// 要素 `id` のポインタ入力から呼ぶ。入ったら `text` を置き、出たら（自分が置いたものなら）消す。
/// `owner` は要素が描かれている間だけ生きている状態（`use_keyed_state` の Entity）。
/// 隣の要素へ移ると、入る・出るの順が前後しても後から入った方が残る。
/// 出たときは、その手が置いた要求だけを消す。もう一方の手は同じ要素を指していることがある。
pub fn hover_tooltip(
    id: impl Into<SharedString>,
    text: SharedString,
    owner: AnyWeakEntity,
    hovered: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let w = window.window_handle().window_id();
    let source = cx.default_global::<TooltipSource>();
    // 配送が終わった後の GPUI の通知には、どの手が動いたかの情報が無い。
    if source.0.is_none() && source.1.contains(&w) {
        return;
    }
    let hand = source.0.unwrap_or(1);
    let id = id.into();
    let map = &mut cx.default_global::<Tooltips>().0;
    if hovered {
        map.insert((w, hand), (id, text, owner));
    } else {
        map.retain(|&(window, source), (i, _, _)| window != w || source != hand || *i != id);
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

/// 箱に収まらない文字を流す速さ（px/秒。1 秒に 3 文字ほど）と、一周ごとに止まる時間（秒）。
pub const SCROLL_SPEED: f32 = font_size::MD * 3.0;
pub const SCROLL_PAUSE: f32 = 1.5;
/// 流すときに並べる 2 つの文字の間。
const SCROLL_GAP: f32 = PAD_X * 2.0;

/// 1 行の幅 `natural`（[`tooltip_width`]）が箱の幅 `width` を超えるとき、出してから `t` 秒後に文字を左へ流した量（px）。
/// 止まる → 後ろに続く 2 つ目の文字が先頭の位置に来るまで流す（一周）→ 止まる、を繰り返す。収まるなら `None`。
pub fn tooltip_scroll(natural: f32, width: f32, t: f32) -> Option<f32> {
    if natural <= width + 0.5 {
        return None;
    }
    let lap = natural - PAD_X * 2.0 + SCROLL_GAP;
    let phase = t % (SCROLL_PAUSE + lap / SCROLL_SPEED);
    Some(((phase - SCROLL_PAUSE).max(0.0) * SCROLL_SPEED).min(lap))
}

/// 幅 `width`（[`tooltip_width`]。動かしている間は途中の値）の箱。収まらない文字は省略せず箱で切る
/// （幅が伸び縮みする間に省略記号が出ないように。中央から開いていくように見える）。
/// `scroll`（[`tooltip_scroll`]）があれば、文字を 2 つ並べて左詰めで流す。
pub fn tooltip(text: SharedString, width: f32, scroll: Option<f32>) -> Div {
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
        .child(match scroll {
            None => div().flex_none().child(text),
            Some(x) => div()
                .flex_none()
                .relative()
                .w(px(width - PAD_X * 2.0))
                .h(px(LINE))
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .left(px(-x))
                        .flex()
                        .gap(px(SCROLL_GAP))
                        .child(div().flex_none().child(text.clone()))
                        .child(div().flex_none().child(text)),
                ),
        })
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
        let rgb = |c: u32| {
            [
                (c >> 24) as f32,
                (c >> 16 & 0xff) as f32,
                (c >> 8 & 0xff) as f32,
            ]
        };
        let bg = rgb(Theme::DARK.tooltip).map(|v| v * a + 255.0 * (1.0 - a));
        let fg = luminance(rgb(Theme::DARK.on_tooltip));
        let ratio = (fg + 0.05) / (luminance(bg) + 0.05);
        assert!(ratio >= 7.0, "{ratio}");
        assert_eq!(Theme::DARK.tooltip, Theme::LIGHT.tooltip);
    }

    #[test]
    fn capsule_ends_clear_the_text() {
        const { assert!(PAD_X >= TOOLTIP_HEIGHT / 2.0 && PAD_X > PAD_Y) };
    }

    #[test]
    fn overflowing_text_pauses_then_scrolls_one_lap() {
        assert_eq!(tooltip_scroll(100.0, 100.0, 9.0), None, "収まれば流さない");
        let (natural, width) = (400.0, 200.0);
        let lap = natural - PAD_X * 2.0 + SCROLL_GAP;
        let period = SCROLL_PAUSE + lap / SCROLL_SPEED;
        assert_eq!(tooltip_scroll(natural, width, 0.0), Some(0.0));
        assert_eq!(tooltip_scroll(natural, width, SCROLL_PAUSE * 0.9), Some(0.0), "初めは止まる");
        let mid = tooltip_scroll(natural, width, SCROLL_PAUSE + 1.0).unwrap();
        assert!((mid - SCROLL_SPEED).abs() < 0.01, "{mid}");
        // 一周の終わりは 2 つ目の文字が先頭の位置にあり、次の始まり（0）と見た目が同じ。
        let end = tooltip_scroll(natural, width, period - 0.001).unwrap();
        assert!((end - lap).abs() < 0.1, "{end}");
        assert_eq!(tooltip_scroll(natural, width, period + 0.1), Some(0.0), "また止まる");
    }

    #[gpui::test]
    fn the_last_entered_element_owns_the_tooltip(cx: &mut gpui::TestAppContext) {
        let w = cx.add_empty_window();
        w.update(|window, cx| {
            let owner = cx.new(|_| ());
            let o = || owner.clone().into_any().downgrade();
            hover_tooltip("a", "A".into(), o(), true, window, cx);
            // 隣へ移ると、入るのが出るより先に届くことがある。
            hover_tooltip("b", "B".into(), o(), true, window, cx);
            hover_tooltip("a", "A".into(), o(), false, window, cx);
            let id = window.window_handle().window_id();
            assert_eq!(cx.global::<Tooltips>().0[&(id, 1)].1, "B");
            hover_tooltip("b", "B".into(), o(), false, window, cx);
            assert!(cx.global::<Tooltips>().0.is_empty());
        });
    }

    #[gpui::test]
    fn each_hand_keeps_its_own_tooltip(cx: &mut gpui::TestAppContext) {
        let w = cx.add_empty_window();
        w.update(|window, cx| {
            let owner = cx.new(|_| ());
            let id = window.window_handle().window_id();
            cx.default_global::<TooltipSource>().0 = Some(0);
            hover_tooltip(
                "shared",
                "Shared".into(),
                owner.clone().into_any().downgrade(),
                true,
                window,
                cx,
            );
            cx.default_global::<TooltipSource>().0 = Some(1);
            hover_tooltip(
                "shared",
                "Shared".into(),
                owner.clone().into_any().downgrade(),
                true,
                window,
                cx,
            );
            assert_eq!(cx.global::<Tooltips>().0[&(id, 0)].1, "Shared");
            assert_eq!(cx.global::<Tooltips>().0[&(id, 1)].1, "Shared");
            hover_tooltip(
                "shared",
                "Shared".into(),
                owner.clone().into_any().downgrade(),
                false,
                window,
                cx,
            );
            assert!(cx.global::<Tooltips>().0.contains_key(&(id, 0)));
            assert!(!cx.global::<Tooltips>().0.contains_key(&(id, 1)));
            cx.default_global::<TooltipSource>().0 = None;
            cx.default_global::<TooltipSource>().1.insert(id);
            hover_tooltip(
                "shared",
                "Deferred".into(),
                owner.into_any().downgrade(),
                true,
                window,
                cx,
            );
            assert!(!cx.global::<Tooltips>().0.contains_key(&(id, 1)));
        });
    }

    #[gpui::test]
    fn a_vanished_element_drops_its_tooltip(cx: &mut gpui::TestAppContext) {
        let w = cx.add_empty_window();
        let owner = w.update(|window, cx| {
            let owner = cx.new(|_| ());
            hover_tooltip(
                "a",
                "A".into(),
                owner.clone().into_any().downgrade(),
                true,
                window,
                cx,
            );
            cx.global_mut::<Tooltips>().drop_stale();
            assert_eq!(cx.global::<Tooltips>().0.len(), 1, "still drawn");
            owner
        });
        // 押して画面が変わり、ホバーが外れた知らせのないまま要素（の状態）が消えた。
        drop(owner);
        w.run_until_parked();
        w.update(|_, cx| {
            cx.global_mut::<Tooltips>().drop_stale();
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
