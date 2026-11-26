//! リンクを開く要求。部品・画面は URL を直接開かず [`open`] で要求するのみ。UI 層が入力配送直後に [`take`] で取り出し、
//! URL をユーザーに確認させてから開く（検問所は `ui::link`。`cx.open_url` を呼べるのはそこだけで、clippy が保護する）。

use gpui::{App, Window, WindowId};

/// 開くよう要求された URL（GPUI グローバル）。要求元ウィンドウごとに保持する。
#[derive(Default)]
struct Requests(Vec<(WindowId, String)>);
impl gpui::Global for Requests {}

/// `url` をブラウザで開くよう要求する。UI 層が開く前にユーザーへ確認を行う。
pub fn open(url: impl Into<String>, window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    cx.default_global::<Requests>().0.push((id, url.into()));
}

/// `window` から要求された URL をすべて取り出す。
pub fn take(window: &Window, cx: &mut App) -> Vec<String> {
    let id = window.window_handle().window_id();
    let requests = &mut cx.default_global::<Requests>().0;
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(requests)
        .into_iter()
        .partition(|(w, _)| *w == id);
    *requests = rest;
    mine.into_iter().map(|(_, url)| url).collect()
}
