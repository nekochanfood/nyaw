//! アプリ終了確認ダイアログ（[`QuitDialog`]）。ダッシュボードのメニューとセットアップの初期画面から起動する。

use design::{Theme, components, components::Variant, icons};
use gpui::{App, Context, Render, Window, prelude::*};
use kernel::{PanelCommand, PanelId, Vec2};

use crate::{PanelControl, PanelSpec};

/// アプリ終了確認ダイアログの起動仕様。表示方法（[`PanelSpec::spawn`] / [`PanelSpec::child`] と配置位置）は
/// `open` が指定されたファクトリで決定する。サイズはテキストに合わせ、ナビゲーションバーは付けない（応答後に閉じるダイアログ）。
pub fn quit_dialog(
    window: &Window,
    cx: &App,
    open: impl FnOnce(fn(PanelId, &mut Window, &mut Context<QuitDialog>) -> QuitDialog) -> PanelSpec,
) -> PanelSpec {
    let width = components::DIALOG_WIDTH;
    let height = components::dialog_height(
        window,
        &i18n::t(cx, "quit.title"),
        &i18n::t(cx, "quit.body"),
    );
    open(|panel, _, _| QuitDialog { panel })
        .sized(
            Vec2::new(width as f32 / 1000.0, height as f32 / 1000.0),
            (width, height),
        )
        .without_nav_bar()
}

/// アプリ終了確認ダイアログ。`quit-confirm` でアプリを終了し、`quit-cancel` ならダイアログのみ閉じる。
pub struct QuitDialog {
    panel: PanelId,
}

impl Render for QuitDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = self.panel;
        components::dialog(
            &theme,
            "quit-dialog",
            &icons::POWER,
            i18n::t(cx, "quit.title"),
            i18n::t(cx, "quit.body"),
            [
                components::button(
                    &theme,
                    "quit-cancel",
                    i18n::t(cx, "common.cancel"),
                    Variant::Default,
                )
                .on_click(move |_, _, cx| crate::command(panel, PanelCommand::Close, cx)),
                components::button(
                    &theme,
                    "quit-confirm",
                    i18n::t(cx, "quit.confirm"),
                    Variant::Danger,
                )
                .on_click(|_, _, cx| cx.default_global::<PanelControl>().quit = true),
            ],
        )
        // 電源アイコンは塗りでは形が潰れて判別しにくいため、線描画にする。
        .weight(design::Weight::Regular)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};

    /// 終了を選択するとアプリを終了し、キャンセルならダイアログのみ閉じる。
    #[gpui::test]
    fn quits_or_cancels(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| QuitDialog { panel: 7 });
        cx.run_until_parked();
        let mut click = |selector| {
            let at = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(at, Modifiers::none());
            cx.update(|_, cx| {
                let control = cx.default_global::<PanelControl>();
                let commands = control.panels.entry(7).or_default().commands.clone();
                (commands, control.quit)
            })
        };
        assert_eq!(click("quit-cancel"), (vec![PanelCommand::Close], false));
        assert!(click("quit-confirm").1);
    }
}
