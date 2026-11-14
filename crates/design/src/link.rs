//! リンクを開く依頼。部品・画面は URL を直接開かず [`open`] で依頼するだけ。UI 層が入力の配送の直後に [`take`] で取り出し、
//! URL を見せてユーザーに確かめてから開く（検問所は `ui::link`。`cx.open_url` を呼べるのはそこだけで、clippy が守る）。

use gpui::{App, Window, WindowId};

/// 開いてほしい URL（GPUI のグローバル）。依頼したウィンドウごと。
#[derive(Default)]
struct Requests(Vec<(WindowId, String)>);
impl gpui::Global for Requests {}

/// `url` をブラウザで開いてほしいと依頼する。開く前に、UI 層がユーザーに確かめる。
pub fn open(url: impl Into<String>, window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    cx.default_global::<Requests>().0.push((id, url.into()));
}

/// `window` から依頼された URL をすべて取り出す。
pub fn take(window: &Window, cx: &mut App) -> Vec<String> {
    let id = window.window_handle().window_id();
    let requests = &mut cx.default_global::<Requests>().0;
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(requests)
        .into_iter()
        .partition(|(w, _)| *w == id);
    *requests = rest;
    mine.into_iter().map(|(_, url)| url).collect()
}
