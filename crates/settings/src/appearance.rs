//! 外観（`settings-appearance`）: テーマの見本・テーマ・パーソナルカラー（セットアップウィザードと同じ部品）。
//! 時計の文字盤は詳しい画面（`settings-watch`）。

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

/// 時計: wrist の文字盤。選択肢は `kernel` の表から並ぶ（足しても、ここは変えない）。
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
