use super::*;
use kernel::subsystems::notifications::Outgoing;

/// OS 通知のテスト画面。ボタン押下で Windows に実際の通知を送信し（PowerShell 名義）、受信した通知がトースト（HUD → 手首 →
/// ダッシュボード）とクイック設定の一覧に表示される。リンク先はすべてこのリポジトリのページ（URL を確認してから開く）。
pub struct NotificationTest;

/// ボタン押下時に開くページの URL。
const REPOSITORY: &str = "https://github.com/nekochanfood/nyaw";
const RELEASES: &str = "https://github.com/nekochanfood/nyaw/releases";

/// テスト用の通知を生成する（タイトル・本文の i18n キーと、ボタン付与の有無）。
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
        let count = cx
            .try_global::<PanelControl>()
            .map_or(0, |c| c.notifications.len());
        let send = |toasts: Vec<Outgoing>| {
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                for toast in &toasts {
                    ui::request(Request::SendNotification(toast.clone()), cx);
                }
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

    /// ボタン押下で Windows への通知送信を要求する（バースト 3 件なら 3 通知）。リンク先は Web ページのみ（アプリは起動しない）。
    #[gpui::test]
    fn buttons_send_test_notifications(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| NotificationTest);
        cx.run_until_parked();
        for (id, n) in [("notify-short", 1), ("notify-long", 1), ("notify-burst", 3)] {
            let at = cx.debug_bounds(id).unwrap().center();
            cx.simulate_click(at, Modifiers::none());
            cx.run_until_parked();
            let sent = cx.update(|_, cx| std::mem::take(&mut cx.default_global::<PanelControl>().requests));
            assert_eq!(sent.len(), n, "{id}");
            for c in sent {
                let Request::SendNotification(t) = c else {
                    panic!("{id}: {c:?}");
                };
                assert!(t.url.starts_with("https://"));
                assert!(t.buttons.iter().all(|(_, url)| url.starts_with("https://")));
            }
        }
    }
}
