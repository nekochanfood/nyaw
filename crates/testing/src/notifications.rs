use super::*;
use kernel::subsystems::notifications::Outgoing;

/// OS の通知を試す画面。押すと Windows に本物の通知を出し（PowerShell の名前で）、届いたものがトースト（HUD → 手首 →
/// ダッシュボード）とクイック設定の一覧に出る。押すと開くものは、どれもこのリポジトリのページ（確かめてから開く）。
pub struct NotificationTest {
    panel: PanelId,
}

impl NotificationTest {
    pub fn new(panel: PanelId) -> Self {
        Self { panel }
    }
}

/// 押すと開くページ。
const REPOSITORY: &str = "https://github.com/nekochanfood/nyaw";
const RELEASES: &str = "https://github.com/nekochanfood/nyaw/releases";

/// テスト用の通知（見出し・本文の i18n のキーと、ボタンを付けるか）。
fn outgoing(cx: &App, title: &str, body: &str, buttons: bool) -> Outgoing {
    Outgoing {
        title: i18n::t(cx, title).to_string(),
        body: i18n::t(cx, body).to_string(),
        url: REPOSITORY.into(),
        buttons: if buttons {
            vec![
                (
                    i18n::t(cx, "notification_test.open_repository").to_string(),
                    REPOSITORY.into(),
                ),
                (
                    i18n::t(cx, "notification_test.open_releases").to_string(),
                    RELEASES.into(),
                ),
            ]
        } else {
            Vec::new()
        },
    }
}

impl Render for NotificationTest {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = self.panel;
        let count = cx
            .try_global::<PanelControl>()
            .map_or(0, |c| c.notifications.len());
        let send = |toasts: Vec<Outgoing>| {
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let commands = toasts.iter().cloned().map(PanelCommand::SendNotification);
                ui::commands(panel, commands, cx);
            }
        };
        let short = outgoing(
            cx,
            "notification_test.short_title",
            "notification_test.short_body",
            false,
        );
        let long = outgoing(
            cx,
            "notification_test.long_title",
            "notification_test.long_body",
            true,
        );
        let burst = (1..=3)
            .map(|n| Outgoing {
                title: i18n::tf(cx, "notification_test.burst_title", &[("n", &n)]).to_string(),
                ..short.clone()
            })
            .collect();
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .p(px(spacing::XL))
            .child(div().child(i18n::t(cx, "notification_test.instructions")))
            .child(
                row()
                    .child(
                        themed_button(
                            &theme,
                            "notify-short",
                            i18n::t(cx, "notification_test.short"),
                            false,
                        )
                        .on_click(send(vec![short.clone()])),
                    )
                    .child(
                        themed_button(
                            &theme,
                            "notify-long",
                            i18n::t(cx, "notification_test.long"),
                            false,
                        )
                        .on_click(send(vec![long])),
                    )
                    .child(
                        themed_button(
                            &theme,
                            "notify-burst",
                            i18n::t(cx, "notification_test.burst"),
                            false,
                        )
                        .on_click(send(burst)),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "notify-count".into())
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::tf(
                        cx,
                        "notification_test.count",
                        &[("count", &count)],
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};

    /// 押すと Windows に通知を出すよう頼む（3 件まとめてなら 3 つ）。押すと開くのはウェブのページだけ（アプリは開かない）。
    #[gpui::test]
    fn buttons_send_test_notifications(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| NotificationTest::new(4));
        cx.run_until_parked();
        for (id, n) in [("notify-short", 1), ("notify-long", 1), ("notify-burst", 3)] {
            let at = cx.debug_bounds(id).unwrap().center();
            cx.simulate_click(at, Modifiers::none());
            cx.run_until_parked();
            let sent = cx.update(|_, cx| {
                let state = cx.default_global::<PanelControl>().panels.entry(4);
                std::mem::take(&mut state.or_default().commands)
            });
            assert_eq!(sent.len(), n, "{id}");
            for c in sent {
                let PanelCommand::SendNotification(t) = c else {
                    panic!("{id}: {c:?}");
                };
                assert!(t.url.starts_with("https://"));
                assert!(t.buttons.iter().all(|(_, url)| url.starts_with("https://")));
            }
        }
    }
}
