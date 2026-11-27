//! 案内（`ui::tour`）が強調する要素の位置。UI 層が強調したい ID を [`want`] で渡すと、その ID のボタン
//! （[`crate::components::Button`]）と [`mark`] を置いた要素が、描画のたびにウィンドウの上の範囲を書き込む。
//! UI 層は同じ描画の中でそれを読み（[`take`]）、輪を描いて、ほかの要素への入力を止める。

use std::collections::HashMap;

use gpui::{App, Bounds, IntoElement, Pixels, SharedString, Window, WindowId, canvas, prelude::*};

/// 見つかった要素 1 つ: ID と、ウィンドウの上の範囲と、角丸の半径。
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub id: SharedString,
    pub bounds: Bounds<Pixels>,
    pub radius: Pixels,
}

/// 強調する ID と、ウィンドウごとに見つかった要素（GPUI のグローバル）。
#[derive(Default)]
pub struct Spots {
    wanted: Vec<SharedString>,
    found: HashMap<WindowId, Vec<Spot>>,
}

impl gpui::Global for Spots {}

/// 強調する ID を差し替える（空にすると何も書き込まない）。
pub fn want(ids: Vec<SharedString>, cx: &mut App) {
    let spots = cx.default_global::<Spots>();
    spots.wanted = ids;
    spots.found.clear();
}

/// `id` が強調する ID か。
pub fn wanted(id: &str, cx: &App) -> bool {
    cx.try_global::<Spots>()
        .is_some_and(|s| s.wanted.iter().any(|w| w == id))
}

/// このウィンドウで、今の描画のうちに書き込まれた要素を取り出す（次の描画でまた書き込まれる）。
pub fn take(window: &Window, cx: &mut App) -> Vec<Spot> {
    let id = window.window_handle().window_id();
    if !cx.has_global::<Spots>() {
        return Vec::new();
    }
    cx.global_mut::<Spots>()
        .found
        .remove(&id)
        .unwrap_or_default()
}

/// 要素 `id` の範囲を書き込む印。強調したい要素の子として置く（親いっぱいに広がり、何も描かない）。
/// `id` が強調する ID でなければ何も置かない。
pub fn mark(id: impl Into<SharedString>, radius: Pixels, cx: &App) -> Option<impl IntoElement> {
    let id = id.into();
    wanted(&id, cx).then(|| {
        canvas(
            move |bounds, window, cx| {
                let at = window.window_handle().window_id();
                let found = cx.default_global::<Spots>().found.entry(at).or_default();
                found.retain(|s| s.id != id);
                found.push(Spot { id, bounds, radius });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    })
}
