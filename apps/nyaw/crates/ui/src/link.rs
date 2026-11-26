//! リンクオープンのゲート。部品・画面からの要求（`design::link::open`）を入力配送直後に取り出し、
//! URL 確認ダイアログを要求元パネルの子として表示する。ブラウザを開くのは確認後のみ（[`open`]）。

use design::{Theme, components, components::Variant, icons};
use gpui::{App, Context, Render, Window, prelude::*};
use kernel::{PanelCommand, PanelId, Vec2};

use crate::{PanelControl, PanelSpec};

/// ブラウザで開くべき URL か判定する。`http` / `https` のみ許可。他のスキーム（`file:` 等）は OS がファイルやアプリを起動するため除外する。
fn browsable(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && ["http", "https"]
                .iter()
                .any(|s| scheme.eq_ignore_ascii_case(s))
    })
}

/// 入力配送直後に呼び出す。`window` から要求された各 URL について、確認ダイアログを `panel` の子として起動する。
pub(crate) fn flush(panel: PanelId, window: &Window, cx: &mut App) {
    for url in design::link::take(window, cx) {
        if !browsable(&url) {
            logger::warn!("ブラウザで開けない URL を破棄した", url = url);
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
            // 応答後に閉じるダイアログ。ナビゲーションバーは不要。
            .without_nav_bar();
        cx.default_global::<PanelControl>().spawns.push(spec);
    }
}

/// ゲートの出口: ユーザーが確認した URL をブラウザで開く。`App::open_url` の呼び出しはここに限定する（`clippy.toml`）。
#[allow(clippy::disallowed_methods)]
fn open(url: &str, cx: &App) {
    cx.open_url(url);
}

/// URL のブラウザオープン確認ダイアログ。URL を省略せず全文表示し、`link-open` で開いて閉じる。
/// `link-cancel` または外側クリックで開かずに閉じる。
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

    /// 要求は直接開かず、要求元パネルの子として確認ダイアログを起動する。ブラウザ対象外の URL では何も起動しない。
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

    /// キャンセルはブラウザを開かず閉じ、「開く」は表示した URL をブラウザで開いて閉じる。
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
