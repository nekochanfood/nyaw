//! 情報（一般カテゴリの `settings-about` から開く詳細画面）: バージョン表示、ライセンス一覧（`setting-licenses`）、
//! リポジトリリンク（`setting-source`、ブラウザで開く）。ライセンス一覧は情報画面の上にスタックする。

use design::components;
use design::{Theme, font_size, radius, spacing};
use gpui::{Context, Div, div, prelude::*, px, rgba, uniform_list};

use crate::{Page, Settings};

// 依存ライセンステーブル `LICENSES`: `(名前, バージョン, ライセンス, URL)`。build.rs がビルド時に生成する。
include!(concat!(env!("OUT_DIR"), "/licenses.rs"));

/// 情報画面のロゴの高さ（px）。
const ABOUT_LOGO_PX: f32 = 144.0;

pub(crate) fn page(s: &Settings, theme: &Theme, cx: &mut Context<Settings>) -> Vec<Div> {
    vec![
        // 先頭にフルサイズのロゴを配置。上下に均等な余白を確保する。
        div()
            .flex()
            .justify_center()
            .py(px(spacing::XL))
            .debug_selector(|| "about-logo".into())
            .child(design::logo(design::Logo::Full).height(px(ABOUT_LOGO_PX))),
        components::list(
            theme,
            [components::menu_item(
                theme,
                "setting-version",
                &design::icons::INFO,
                i18n::t(cx, "settings.version"),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(rgba(theme.neutral_text_low))
                    .child(updater::VERSION),
            )],
        ),
        // バージョン行とライセンス・ソースコード行の間の区切り線。
        div().h(px(1.0)).bg(rgba(theme.neutral_border_subtle)),
        components::list(
            theme,
            [
                s.page_row(theme, Page::Licenses, cx),
                components::menu_item(
                    theme,
                    "setting-source",
                    &design::icons::CODE,
                    i18n::t(cx, "settings.source_code"),
                )
                .child(external_icon(theme))
                .on_click(|_, window, cx| {
                    design::link::open(env!("CARGO_PKG_REPOSITORY"), window, cx)
                }),
            ],
        ),
    ]
}

/// ライセンス一覧: 依存ごとに 1 行を表示し、クリックでリポジトリをブラウザで開く。数百行に及ぶため
/// `uniform_list` による仮想化で可視行のみを生成する（行の高さは均一。先頭行の区切り線のみ透明にして高さを維持）。
pub(crate) fn licenses(s: &Settings, theme: &Theme) -> Vec<Div> {
    let last = LICENSES.len() - 1;
    let rows = uniform_list("licenses", LICENSES.len(), move |range, _, cx| {
        let theme = Theme::current(cx);
        range
            .map(|i| {
                let (name, version, license, url) = LICENSES[i];
                let mut row = components::menu_item(
                    &theme,
                    format!("license-{name}-{version}"),
                    &design::icons::PACKAGE,
                    name,
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(font_size::SM))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(format!("{version} · {license}")),
                )
                .child(external_icon(&theme))
                .on_click(move |_, window, cx| design::link::open(url, window, cx));
                if i == 0 {
                    row = row.rounded_t(px(radius::LG));
                }
                if i == last {
                    row = row.rounded_b(px(radius::LG));
                }
                let line = if i == 0 {
                    0
                } else {
                    theme.neutral_border_subtle
                };
                div().border_t_1().border_color(rgba(line)).child(row)
            })
            .collect()
    });
    let bars = s.licenses_scroll.0.borrow().base_handle.clone();
    vec![
        div().flex_1().min_h_0().child(
            components::card(theme)
                .relative()
                .size_full()
                .child(rows.track_scroll(&s.licenses_scroll).size_full())
                .child(
                    components::scrollbars(theme, "licenses-bars", &bars).corner_radius(radius::LG),
                ),
        ),
    ]
}

/// 外部リンク（ブラウザで開く）行の右端に表示するアイコン。
fn external_icon(theme: &Theme) -> impl IntoElement {
    design::icon(&design::icons::ARROW_SQUARE_OUT)
        .size(px(font_size::MD))
        .color(rgba(theme.neutral_text_low))
}
