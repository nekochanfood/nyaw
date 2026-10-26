//! 効果音の要求。部品は「何が起きたか」（[`Cue`]）をウィンドウに置くだけで、どの音をどこから鳴らすかは知らない。
//! UI 層が入力の配送の直後に [`take`] で取り出し、そのパネルのスピーカーへ渡す（`ui::cue`）。
//! 新しい種類は [`Cue`] に足す。音の割り当て（`ui::cue::sound`）が網羅の `match` なので、足し忘れるとビルドが落ちる。

use std::{collections::HashMap, time::Duration, time::Instant};

use gpui::{App, Pixels, Point, SharedString, Window, WindowId};

use crate::components::TooltipSource;

/// 効果音の種類。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cue {
    /// 押せる要素にポインタが入った。
    Hover,
    /// 押せる要素を押した。
    Click,
}

impl Cue {
    pub const ALL: [Cue; 2] = [Cue::Hover, Cue::Click];

    /// 同じ種類の音の最短の間隔。続けて鳴ると聞こえが悪くなる。
    pub const fn min_gap(self) -> Duration {
        match self {
            Cue::Hover | Cue::Click => Duration::from_millis(50),
        }
    }
}

/// 鳴らしてほしい音の要求（GPUI のグローバル）。ウィンドウごとに、種類と鳴らす点（ウィンドウのピクセル）。
#[derive(Default)]
struct Cues {
    queue: Vec<(WindowId, Cue, Point<Pixels>)>,
    last: HashMap<Cue, Instant>,
}
impl gpui::Global for Cues {}

/// ポインタの位置から `cue` を鳴らしてほしいと要求する。直前の同じ種類から [`Cue::min_gap`] 以内なら捨てる。
pub fn emit(cue: Cue, window: &Window, cx: &mut App) {
    let now = crate::drive::now();
    let cues = cx.default_global::<Cues>();
    if cues
        .last
        .get(&cue)
        .is_some_and(|&t| now.saturating_duration_since(t) < cue.min_gap())
    {
        return;
    }
    cues.last.insert(cue, now);
    cues.queue
        .push((window.window_handle().window_id(), cue, window.mouse_position()));
}

/// `window` 宛ての要求をすべて取り出す。
pub fn take(window: &Window, cx: &mut App) -> Vec<(Cue, Point<Pixels>)> {
    let id = window.window_handle().window_id();
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut cx.default_global::<Cues>().queue)
        .into_iter()
        .partition(|c| c.0 == id);
    cx.default_global::<Cues>().queue = rest;
    mine.into_iter().map(|(_, cue, at)| (cue, at)).collect()
}

/// 手ごとの、ホバー中の要素（ウィンドウごと）。GPUI のホバーはウィンドウに一つで、片手が押している間などは入る・出るが
/// 届かないので、手ごとの移動（`on_mouse_move`）で見る。`now` はこの移動で指している要素、`prev` は一つ前の移動のもの。
/// 前の移動で指していなかった要素に入ったときだけ鳴らす。
#[derive(Default)]
struct Hovers {
    now: HashMap<(WindowId, u8), SharedString>,
    prev: HashMap<(WindowId, u8), SharedString>,
}
impl gpui::Global for Hovers {}

/// 手 `hand`（0 = 左、1 = 右）の移動を配送する前に呼ぶ。
pub fn begin_hover(window: &Window, hand: u8, cx: &mut App) {
    let key = (window.window_handle().window_id(), hand);
    let h = cx.default_global::<Hovers>();
    h.prev.remove(&key);
    if let Some(id) = h.now.remove(&key) {
        h.prev.insert(key, id);
    }
}

/// 要素 `id` をポインタが指していると知らせる（入力のたびに呼んでよい）。新しく入ったときだけ [`Cue::Hover`] を要求する。
pub fn hover(id: &SharedString, window: &Window, cx: &mut App) {
    let w = window.window_handle().window_id();
    let source = cx.default_global::<TooltipSource>();
    // 配送が終わった後の GPUI の通知には、どの手が動いたかの情報が無い。
    if source.0.is_none() && source.1.contains(&w) {
        return;
    }
    let key = (w, source.0.unwrap_or(1));
    let h = cx.default_global::<Hovers>();
    if h.now.get(&key) == Some(id) {
        return;
    }
    h.now.insert(key, id.clone());
    if h.prev.get(&key) != Some(id) {
        emit(Cue::Hover, window, cx);
    }
}
