//! 入力（`settings-input`）: 利き手（主に操作する手）・ダッシュボード中のゲーム入力の遮断。
//! ポインタとキーボードは詳しい画面（`pointer.rs` / `keyboard.rs`）。

use design::Theme;
use design::components;
use gpui::{Context, Div, Window, prelude::*};
use kernel::{DominantHand, Setting};

use crate::{Page, Settings, change};

pub(crate) fn page(
    s: &Settings,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> Vec<Div> {
    let now = ui::settings(cx);
    let hand = s.select(
        theme,
        window,
        cx,
        "setting-dominant-hand",
        &design::icons::HAND,
        i18n::t(cx, "settings.dominant_hand"),
        now.dominant_hand,
        vec![
            (
                "setting-hand-right",
                i18n::t(cx, "settings.right"),
                DominantHand::Right,
            ),
            (
                "setting-hand-left",
                i18n::t(cx, "settings.left"),
                DominantHand::Left,
            ),
        ],
        Setting::DominantHand,
    );
    let block = components::switch_item(
        theme,
        "setting-block-input",
        &design::icons::PROHIBIT,
        i18n::t(cx, "settings.block_input"),
        now.block_input,
    )
    .on_click(change(Setting::BlockInput(!now.block_input)));
    vec![
        components::list(theme, [hand, block]),
        s.page_rows(theme, [Page::Pointer, Page::Keyboard], cx),
    ]
}
