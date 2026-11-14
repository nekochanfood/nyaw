//! リンクを開く検問所。部品・画面からの依頼（`design::link::open`）を入力の配送の直後に取り出し、URL を見せて確かめる
//! ダイアログを依頼元のパネルの子として開く。ブラウザを開くのは、そこで「開く」を押したときだけ（[`open`]）。

use design::{Theme, components, components::Variant, icons};
use gpui::{App, Context, Render, Window, prelude::*};
use kernel::{PanelCommand, PanelId, Vec2};

use crate::{PanelControl, PanelSpec};

/// ブラウザで開くものか: `http` / `https` だけ。ほかのスキーム（`file:` など）は OS がファイルやアプリを開いてしまう。
fn browsable(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && ["http", "https"]
                .iter()
                .any(|s| scheme.eq_ignore_ascii_case(s))
    })
}

/// 入力を配送した直後に呼ぶ。`window` から依頼された URL ごとに、確かめるダイアログを `panel` の子として開く。
pub(crate) fn flush(panel: PanelId, window: &Window, cx: &mut App) {
    for url in design::link::take(window, cx) {
        if !browsable(&url) {
            logger::warn!("ブラウザで開くものではない URL を捨てた", url = url);
            continue;
        }
        let title = i18n::t(cx, "link.title");
        let (width, height) = (
            components::DIALOG_WIDTH,
            components::dialog_height(window, &title, &url),
        );
        let spec = PanelSpec::child(panel, true, move |panel, _, _| LinkDialog { panel, url })
            .sized(
                Vec2::new(width as f32 / 1000.0, height as f32 / 1000.0),
                (width, height),
            )
            // 答えて閉じるもの。ナビゲーションバーは付けない。
            .without_nav_bar();
        cx.default_global::<PanelControl>().spawns.push(spec);
    }
}

/// 検問所の出口: ユーザーが確かめた URL をブラウザで開く。`App::open_url` を呼んでよいのはここだけ（`clippy.toml`）。
#[allow(clippy::disallowed_methods)]
fn open(url: &str, cx: &App) {
    cx.open_url(url);
}

/// URL をブラウザで開いてよいか確かめるダイアログ。URL を省かずにそのまま見せ、`link-open` で開いて閉じる。
/// `link-cancel` か外側を押すと開かずに閉じる。
struct LinkDialog {
    panel: PanelId,
    url: String,
}

impl Render for LinkDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (panel, url) = (self.panel, self.url.clone());
        components::dialog(
            &theme,
            "link-dialog",
            &icons::ARROW_SQUARE_OUT,
            i18n::t(cx, "link.title"),
            self.url.clone(),
            [
                components::button(
                    &theme,
                    "link-cancel",
                    i18n::t(cx, "common.cancel"),
                    Variant::Default,
                )
                .on_click(move |_, _, cx| crate::command(panel, PanelCommand::Close, cx)),
                components::button(
                    &theme,
                    "link-open",
                    i18n::t(cx, "link.open"),
                    Variant::Primary,
                )
                .on_click(move |_, _, cx| {
                    open(&url, cx);
                    crate::command(panel, PanelCommand::Close, cx);
                }),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};

    #[test]
    fn only_web_urls_go_to_the_browser() {
        assert!(browsable("https://github.com/nekochanfood/nyaw"));
        assert!(browsable("HTTP://example.com"));
        assert!(!browsable("file:///C:/Windows/System32/calc.exe"));
        assert!(!browsable("ms-settings:display"));
        assert!(!browsable("javascript://alert(1)"));
        assert!(!browsable("https://"));
        assert!(!browsable("C:\\Windows\\notepad.exe"));
    }

    /// 依頼は開かず、依頼元のパネルの子として確かめるダイアログを開く。ブラウザで開くものでなければ何も開かない。
    #[gpui::test]
    fn requests_open_a_confirmation_instead_of_the_browser(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| gpui::Empty);
        let spawned = |cx: &mut VisualTestContext, url: &'static str| {
            cx.update(|window, cx| {
                design::link::open(url, window, cx);
                flush(3, window, cx);
                std::mem::take(&mut cx.default_global::<PanelControl>().spawns)
            })
        };
        let specs = spawned(cx, "https://example.com/notes");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].parent().map(|p| p.panel), Some(3));
        assert!(spawned(cx, "file:///C:/x.exe").is_empty());
        assert_eq!(cx.opened_url(), None);
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let at = cx.debug_bounds(selector).unwrap().center();
        cx.simulate_click(at, Modifiers::none());
    }

    fn commands(cx: &mut VisualTestContext) -> Vec<PanelCommand> {
        cx.update(|_, cx| {
            std::mem::take(
                &mut cx
                    .default_global::<PanelControl>()
                    .panels
                    .entry(7)
                    .or_default()
                    .commands,
            )
        })
    }

    /// キャンセルは開かずに閉じ、「開く」は見せた URL をブラウザで開いて閉じる。
    #[gpui::test]
    fn the_dialog_opens_only_what_it_shows(cx: &mut TestAppContext) {
        let url = "https://github.com/nekochanfood/nyaw/releases/tag/v2026.0.2";
        let (_, cx) = cx.add_window_view(|_, _| LinkDialog {
            panel: 7,
            url: url.into(),
        });
        cx.run_until_parked();
        click(cx, "link-cancel");
        assert_eq!(commands(cx), [PanelCommand::Close]);
        assert_eq!(cx.opened_url(), None);
        click(cx, "link-open");
        assert_eq!(commands(cx), [PanelCommand::Close]);
        assert_eq!(cx.opened_url().as_deref(), Some(url));
    }
}
