//! テスト共通ユーティリティ。テストは各モジュールの子（`pointer/tests.rs` 等）に配置する。

use super::*;

pub(crate) use design::Theme;
pub(crate) use gpui::{TestAppContext, div};
pub(crate) use kernel::Runtime;

/// UI 層単体テスト用の最小画面。
#[derive(Default)]
pub(crate) struct Probe {
    pub(crate) clicks: usize,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("probe")
                .debug_selector(|| "probe".into())
                .size_10()
                .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
        )
    }
}
