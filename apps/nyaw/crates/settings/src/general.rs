//! 一般（`settings-general`）: 言語選択（各言語名はその言語自身で表記し、どの表示言語でも判読可能）、
//! SteamVR 自動起動（切替可能なランタイムの場合のみ表示）。アップデートと情報は詳細画面に分離。

use design::Theme;
use design::components;
use gpui::{Context, Div, SharedString, Window, prelude::*};
use kernel::Setting;

use crate::{Page, Settings, change};

pub(crate) fn page(
    s: &Settings,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> Vec<Div> {
    let now = ui::settings(cx);
    let language = s.select(
        theme,
        window,
        cx,
        "setting-language",
        &design::icons::TRANSLATE,
        i18n::t(cx, "settings.language"),
        ui::lang(now.language),
        i18n::Lang::ALL
            .map(|l| {
                let id = match l {
                    i18n::Lang::En => "setting-lang-en",
                    i18n::Lang::Ja => "setting-lang-ja",
                };
                (id, SharedString::from(l.native_name()), l)
            })
            .to_vec(),
        |l| Setting::Language(ui::language(l)),
    );
    let auto_launch = now.auto_launch.map(|on| {
        components::switch_item(
            theme,
            "setting-auto-launch",
            &design::icons::ROCKET_LAUNCH,
            i18n::t(cx, "settings.auto_launch"),
            on,
        )
        .on_click(change(Setting::AutoLaunch(!on)))
    });
    vec![
        components::list(theme, std::iter::once(language).chain(auto_launch)),
        s.page_rows(theme, [Page::Update, Page::About], cx),
    ]
}
