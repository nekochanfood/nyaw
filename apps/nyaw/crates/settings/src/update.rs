//! アップデート（一般カテゴリの `settings-update` から開く詳細画面）: 現在バージョンと確認状態、チャンネル（安定版 / ナイトリー）、
//! 新リリースがあればバージョン・アップデートボタン・リリースノート（Markdown）を表示する。
//! 確認・ダウンロード・インストールは `updater` クレートに委譲する。自動インストールは行わず、ユーザー操作時のみ実行する。

use std::sync::atomic::Ordering;

use design::components::{self, Variant};
use design::{Theme, font_size, spacing};
use gpui::{App, Context, Div, FontWeight, SharedString, Window, div, prelude::*, px, rgba};
use kernel::{Setting, UpdateChannel};
use updater::Step;

use crate::Settings;

/// チャンネル選択（安定版は正式リリースのみ、ナイトリーはプレリリースを含む）。
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

/// ページ本体（グループの並び）。`channel` はチャンネル選択行。
/// 確認中・ダウンロード中は描画を継続し、完了と進捗を検出する。
fn body(theme: &Theme, channel: components::Button, window: &mut Window, cx: &mut App) -> Vec<Div> {
    // 推奨リリースはチャンネルにより異なる（切替時に即時反映、再確認不要）。
    let latest = updater::latest(cx);
    let u = updater::state(cx);
    let (step, error, checked) = (u.step.clone(), u.error.clone(), u.checked);
    if matches!(step, Step::Checking | Step::Downloading(_) | Step::Applying) {
        components::keep_drawing(window, cx);
    }
    let low = rgba(theme.neutral_text_low);
    // ビルド番号は新旧比較専用であり、画面にはバージョン文字列のみ表示する。
    let current = updater::VERSION;
    // 確認結果の表示。エラーは新リリースが存在すればインストール時のもの（下部に表示）、なければ確認時のもの。
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
    // インストール操作: ボタン → ダウンロード中はプログレスリングと進捗率 → インストール実行後は再起動待ちテキスト。
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
        Step::Applying => div()
            .debug_selector(|| "update-applying".into())
            .child(i18n::t(cx, "update.applying")),
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
