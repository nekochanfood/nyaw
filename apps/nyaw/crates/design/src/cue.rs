//! 効果音の要求。部品は「何が起きたか」（[`Cue`]）をウィンドウに登録するだけで、どの音をどこから再生するかは関知しない。
//! UI 層が入力配送直後に [`take`] で取り出し、そのパネルのスピーカーへ渡す（`ui::cue`）。
//! 新しい種類は [`Cue`] に追加する。音の割り当て（`ui::cue::sound`）が網羅 `match` のため、追加漏れはビルドエラーになる。

use std::{collections::HashMap, time::Duration, time::Instant};

use gpui::{App, Pixels, Point, SharedString, Window, WindowId};

use crate::components::TooltipSource;

/// 効果音の種別。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cue {
    /// 操作可能な要素にポインタが進入した。
    Hover,
    /// 操作可能な要素を押下した。
    Click,
}

impl Cue {
    pub const ALL: [Cue; 2] = [Cue::Hover, Cue::Click];

    /// 同一種別の音の最短間隔。連続再生すると聴感が悪化するため。
    pub const fn min_gap(self) -> Duration {
        match self {
            Cue::Hover | Cue::Click => Duration::from_millis(50),
        }
    }
}

/// 再生要求された音（GPUI グローバル）。ウィンドウごとに種別と再生位置（ウィンドウ座標のピクセル）を保持する。
#[derive(Default)]
struct Cues {
    queue: Vec<(WindowId, Cue, Point<Pixels>)>,
    last: HashMap<Cue, Instant>,
}
impl gpui::Global for Cues {}

/// ポインタの位置から `cue` の再生を要求する。直前の同一種別から [`Cue::min_gap`] 以内の場合は破棄する。
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
    cues.queue.push((
        window.window_handle().window_id(),
        cue,
        window.mouse_position(),
    ));
}

/// `window` 宛ての要求をすべて取り出して返す。
pub fn take(window: &Window, cx: &mut App) -> Vec<(Cue, Point<Pixels>)> {
    let id = window.window_handle().window_id();
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut cx.default_global::<Cues>().queue)
        .into_iter()
        .partition(|c| c.0 == id);
    cx.default_global::<Cues>().queue = rest;
    mine.into_iter().map(|(_, cue, at)| (cue, at)).collect()
}

/// ハンドごとのホバー中要素（ウィンドウごと）。GPUI のホバーはウィンドウにつき一つで、片手が押下中の場合等は enter/leave が
/// 届かないため、ハンドごとの移動（`on_mouse_move`）で監視する。`now` は今回の移動で指している要素、`prev` は前回の移動のもの。
/// 前回の移動で指していなかった要素に進入したときのみ再生する。
#[derive(Default)]
struct Hovers {
    now: HashMap<(WindowId, u8), SharedString>,
    prev: HashMap<(WindowId, u8), SharedString>,
}
impl gpui::Global for Hovers {}

/// ハンド `hand`（0 = 左、1 = 右）の移動を配送する前に呼び出す。
pub fn begin_hover(window: &Window, hand: u8, cx: &mut App) {
    let key = (window.window_handle().window_id(), hand);
    let h = cx.default_global::<Hovers>();
    h.prev.remove(&key);
    if let Some(id) = h.now.remove(&key) {
        h.prev.insert(key, id);
    }
}

/// 要素 `id` をポインタが指していることを通知する（入力のたびに呼んでよい）。新規進入時のみ [`Cue::Hover`] を要求する。
pub fn hover(id: &SharedString, window: &Window, cx: &mut App) {
    let w = window.window_handle().window_id();
    let source = cx.default_global::<TooltipSource>();
    // 配送完了後の GPUI の通知には、どのハンドが移動したかの情報がない。
    if source.0.is_none() && source.1.contains(&w) {
        return;
    }
    let (hand, key) = (source.0, (w, source.0.unwrap_or(1)));
    let h = cx.default_global::<Hovers>();
    match h.now.get(&key) {
        Some(now) if now == id => return,
        // 1 回の移動は内側の要素から順に到達する。外側（内部にボタンを含むカード等）で上書きすると、次の移動で内側に
        // 再進入した扱いになり鳴り続けるため、先に受けた内側の要素を維持する。
        Some(_) if hand.is_some() => return,
        _ => {}
    }
    h.now.insert(key, id.clone());
    if h.prev.get(&key) != Some(id) {
        emit(Cue::Hover, window, cx);
    }
}
