//! 一般（`settings-general`）: 言語（名前はその言語自身で出す。どの言語で表示していても読める）、SteamVR と一緒に起動
//! （切り替えられるランタイムのときだけ）。アップデートと情報は詳しい画面。

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
