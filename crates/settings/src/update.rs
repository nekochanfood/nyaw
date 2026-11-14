//! 設定の「アップデート」（一般の行 `settings-update` から開く詳しい画面）: 今の版と確認の状態、チャンネル（安定版 / ナイトリー）、新しいリリースが
//! あればその版・アップデートのボタン・リリースノート（Markdown）。確かめる・落として入れるのは `updater`。
//! 勝手には入れず、ユーザーが押したときだけ入れる。

use std::sync::atomic::Ordering;

use design::components::{self, Variant};
use design::{Theme, font_size, spacing};
use gpui::{App, Context, Div, FontWeight, SharedString, Window, div, prelude::*, px, rgba};
use kernel::{Setting, UpdateChannel};
use updater::Step;

use crate::Settings;

/// チャンネル（安定版は正式なリリースだけ、ナイトリーはプレリリースも）はここで選ぶ。
pub(crate) fn page(
    s: &Settings,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> Vec<Div> {
    let channel = s.select(
        theme,
        window,
        cx,
        "setting-update-channel",
        &design::icons::GIT_BRANCH,
        i18n::t(cx, "update.channel"),
        ui::settings(cx).update_channel,
        vec![
            (
                "setting-channel-stable",
                i18n::t(cx, "update.stable"),
                UpdateChannel::Stable,
            ),
            (
                "setting-channel-nightly",
                i18n::t(cx, "update.nightly"),
                UpdateChannel::Nightly,
            ),
        ],
        Setting::UpdateChannel,
    );
    body(theme, channel, window, cx)
}

/// 中身（グループの並び）。`channel` はチャンネルを選ぶ行。
/// 確かめている・落としている間は描き続けて、終わりと進み具合を拾う。
fn body(theme: &Theme, channel: components::Button, window: &mut Window, cx: &mut App) -> Vec<Div> {
    // 勧めるリリースはチャンネルで変わる（切り替えたらすぐ。確かめ直さない）。
    let latest = updater::latest(cx);
    let u = updater::state(cx);
    let (step, error, checked) = (u.step.clone(), u.error.clone(), u.checked);
    if matches!(step, Step::Checking | Step::Downloading(_)) {
        components::keep_drawing(window, cx);
    }
    let low = rgba(theme.neutral_text_low);
    // ビルド番号は新旧を比べるためだけのもので、画面には出さない（版だけ）。
    let current = updater::VERSION;
    // 確かめた結果。失敗は、新しいリリースがあれば入れるときのもの（下に出す）、無ければ確かめたときのもの。
    let (status, failed) = match (&step, &latest, &error) {
        (Step::Checking, ..) => (i18n::t(cx, "update.checking"), false),
        (_, None, Some(e)) => (i18n::tf(cx, "update.failed", &[("error", e)]), true),
        (_, Some(_), _) => (i18n::t(cx, "update.available"), false),
        (_, None, None) if checked => (i18n::t(cx, "update.up_to_date"), false),
        _ => (i18n::t(cx, "update.not_checked"), false),
    };
    let row = || {
        div()
            .flex()
            .items_center()
            .gap(px(spacing::MD))
            .px(px(spacing::LG))
            .py(px(spacing::MD))
    };
    let summary = components::card(theme)
        .flex()
        .flex_col()
        .child(
            row()
                .debug_selector(|| "update-current".into())
                .child(design::icon(&design::icons::INFO).size(px(font_size::LG)))
                .child(div().flex_1().child(i18n::t(cx, "update.current")))
                .child(div().text_color(low).child(current)),
        )
        .child(div().h(px(1.0)).bg(rgba(theme.neutral_border_subtle)))
        .child(
            row()
                .child(design::icon(&design::icons::ARROWS_CLOCKWISE).size(px(font_size::LG)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .debug_selector(|| "update-status".into())
                        .when(failed, |d| d.text_color(rgba(theme.danger_text)))
                        .child(status),
                )
                .child(
                    components::button(
                        theme,
                        "update-check",
                        i18n::t(cx, "update.check"),
                        Variant::Default,
                    )
                    .on_click(|_, _, cx| updater::check_now(cx)),
                ),
        );
    let channel = components::list(theme, [channel]);
    let Some(release) = latest else {
        return vec![summary, channel];
    };
    // 入れる操作: ボタン → 落としている間は進み具合の輪と割合 → 走らせたら再起動を待つ文字。
    let action = match &step {
        Step::Downloading(done) => {
            let p = done.load(Ordering::Relaxed) as f32 / release.installer.size.max(1) as f32;
            div()
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .debug_selector(|| "update-progress".into())
                .child(components::progress_ring(theme, 24.0, 3.0, p))
                .child(i18n::tf(
                    cx,
                    "update.downloading",
                    &[("percent", &format!("{:.0}", p * 100.0))],
                ))
        }
        Step::Installing => div()
            .debug_selector(|| "update-installing".into())
            .child(i18n::t(cx, "update.installing")),
        _ => div().child(
            components::button(
                theme,
                "update-install",
                i18n::t(cx, "update.install"),
                Variant::Primary,
            )
            .on_click(|_, _, cx| updater::install(cx)),
        ),
    };
    let date = release
        .published_at
        .as_deref()
        .and_then(|d| d.get(..10))
        .unwrap_or_default();
    let meta = i18n::tf(cx, "update.published", &[("date", &date)]);
    let page = release.page.clone();
    let card = components::card(theme)
        .flex()
        .flex_col()
        .gap(px(spacing::LG))
        .p(px(spacing::LG))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(spacing::MD))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(font_size::LG))
                                .font_weight(FontWeight::BOLD)
                                .debug_selector(|| "update-version".into())
                                .child(SharedString::from(format!("nyaw {}", release.version))),
                        )
                        .child(
                            div()
                                .text_size(px(font_size::SM))
                                .text_color(low)
                                .child(meta),
                        ),
                )
                .child(action),
        )
        .children(
            error
                .filter(|_| !matches!(step, Step::Downloading(_)))
                .map(|e| {
                    div()
                        .text_color(rgba(theme.danger_text))
                        .debug_selector(|| "update-error".into())
                        .child(i18n::tf(cx, "update.install_failed", &[("error", &e)]))
                }),
        )
        .child(div().h(px(1.0)).bg(rgba(theme.neutral_border_subtle)))
        .child(
            div()
                .debug_selector(|| "update-notes".into())
                .child(components::markdown(
                    theme,
                    "update-notes",
                    &release.notes,
                    window,
                    cx,
                )),
        );
    vec![
        summary,
        channel,
        components::group(theme, i18n::t(cx, "update.new_version"), card),
        components::list(
            theme,
            [components::menu_item(
                theme,
                "update-page",
                &design::icons::ARROW_SQUARE_OUT,
                i18n::t(cx, "update.release_page"),
            )
            .on_click(move |_, window, cx| design::link::open(page.clone(), window, cx))],
        ),
    ]
}
