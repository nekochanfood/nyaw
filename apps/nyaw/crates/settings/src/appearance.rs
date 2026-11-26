//! 外観（`settings-appearance`）: テーマプレビュー・テーマ選択・パーソナルカラー（セットアップウィザードと共通の部品）。
//! 時計の文字盤は詳細画面（`settings-watch`）に分離。

use design::Theme;
use gpui::{App, Context, Div, Window};

use crate::{Page, Settings};

pub(crate) fn page(
    s: &Settings,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> Vec<Div> {
    let this = cx.entity();
    let set = move |state, _: &mut Window, cx: &mut App| {
        this.update(cx, |v, cx| {
            v.appearance = state;
            cx.notify();
        })
    };
    vec![
        ui::prefs::appearance(s.panel, s.appearance, set, window, cx),
        s.page_rows(theme, [Page::Watch], cx),
    ]
}

/// 時計の文字盤選択。選択肢は `kernel` のテーブルから生成される（文字盤追加時もここの変更は不要）。
pub(crate) fn watch(s: &Settings, window: &mut Window, cx: &mut Context<Settings>) -> Vec<Div> {
    let this = cx.entity();
    let set = move |state, _: &mut Window, cx: &mut App| {
        this.update(cx, |v, cx| {
            v.watch = state;
            cx.notify();
        })
    };
    vec![ui::prefs::watch_faces(s.watch, set, window, cx)]
}
