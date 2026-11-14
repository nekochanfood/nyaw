//! ローカライズを確かめる画面（[`LocaleTest`]）。

use super::*;

/// ローカライズを確かめる画面（`locale-test`）。
/// - `lang-en` / `lang-ja` で言語を切り替える（全パネルに効き、設定ファイルの `language` に残る。`Setting::Language`）。
///   ボタンの名前はその言語自身で出す。今の言語のボタンが光る。
/// - `locale-current` に今の言語、`locale-missing` に訳の抜け（言語ごとに表に無いキー）を出す。
/// - 見本（`locale-samples`）: 長い文の折り返し、差し込み（`{count}`）、訳で長さの変わるボタン。
/// - `locale-table` に全キーを、言語ごとの文言と並べて出す（今の言語の列を強調）。抜けは赤字で `—`。
pub struct LocaleTest {
    pub table: ScrollHandle,
}

impl LocaleTest {
    pub fn new(_panel: PanelId) -> Self {
        Self {
            table: ScrollHandle::new(),
        }
    }
}

impl Render for LocaleTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let now = settings(cx).language;
        let current = ui::lang(now);
        let switch = row().children(i18n::Lang::ALL.map(|l| {
            let id = match l {
                i18n::Lang::En => "lang-en",
                i18n::Lang::Ja => "lang-ja",
            };
            button(&theme, id, l.native_name(), l == current)
                .on_click(change(Setting::Language(ui::language(l))))
        }));
        let missing: Vec<(i18n::Lang, Vec<&'static str>)> = i18n::Lang::ALL
            .into_iter()
            .map(|l| (l, i18n::missing(l)))
            .filter(|(_, keys)| !keys.is_empty())
            .collect();
        let missing = div()
            .debug_selector(|| "locale-missing".into())
            .flex()
            .flex_col()
            .gap(px(spacing::XS))
            .when(missing.is_empty(), |d| {
                d.text_color(rgba(theme.neutral_text_low))
                    .child(i18n::t(cx, "locale.complete"))
            })
            .children(missing.into_iter().map(|(l, keys)| {
                div()
                    .text_color(rgba(theme.danger_text))
                    .child(i18n::tf(
                        cx,
                        "locale.missing",
                        &[("language", &l.native_name()), ("count", &keys.len())],
                    ))
                    .child(div().text_size(px(font_size::SM)).child(keys.join(", ")))
            }));
        let samples = div()
            .debug_selector(|| "locale-samples".into())
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .child(div().child(i18n::t(cx, "locale.sample_long")))
            .child(div().child(i18n::tf(cx, "locale.sample_count", &[("count", &3)])))
            .child(row().child(button(
                &theme,
                "locale-sample-button",
                i18n::t(cx, "locale.sample_button"),
                false,
            )));
        // 全キーの表: キーと、言語ごとの文言。
        let cell = |text: SharedString, strong: bool| {
            div()
                .flex_1()
                .min_w_0()
                .px(px(spacing::SM))
                .when(!strong, |d| d.text_color(rgba(theme.neutral_text_low)))
                .child(text)
        };
        let header = div()
            .flex()
            .py(px(spacing::XS))
            .font_weight(FontWeight::BOLD)
            .child(cell(i18n::t(cx, "locale.key"), true))
            .children(i18n::Lang::ALL.map(|l| cell(l.native_name().into(), l == current)));
        let rows = i18n::keys().into_iter().map(|key| {
            div()
                .flex()
                .py(px(spacing::XS))
                .text_size(px(font_size::SM))
                .child(cell(key.into(), false))
                .children(i18n::Lang::ALL.map(|l| match i18n::get(l, key) {
                    Some(text) => cell(text.into(), l == current),
                    None => cell("—".into(), true).text_color(rgba(theme.danger_text)),
                }))
        });
        let table = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("locale-table")
                    .debug_selector(|| "locale-table".into())
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.table)
                    .child(
                        components::elastic("pull-locale", &self.table)
                            .flex()
                            .flex_col()
                            .child(header)
                            .children(rows),
                    ),
            )
            .child(
                components::scrollbars(&theme, "locale-bars", &self.table).pulled_by("pull-locale"),
            );
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .p(px(spacing::XL))
            .text_color(rgba(theme.neutral_text))
            .child(switch)
            .child(
                div()
                    .debug_selector(|| "locale-current".into())
                    .child(i18n::tf(
                        cx,
                        "locale.current",
                        &[("language", &current.native_name())],
                    )),
            )
            .child(missing)
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(i18n::t(cx, "locale.samples")),
            )
            .child(samples)
            .child(table)
    }
}

#[cfg(test)]
mod tests;
