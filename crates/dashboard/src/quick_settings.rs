//! クイック設定: 上に時刻・日付と設定ボタン、その下に届いた OS の通知の一覧（新しい順。カードは通知のトーストと同じ
//! `ui::notification::card`。ホバーで広げ、押すと開く）。

use super::*;

impl Screen {
    pub(crate) fn quick_settings(
        &mut self,
        theme: &Theme,
        time_width: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Div {
        let now = chrono::Local::now();
        let notes = cx.global::<PanelControl>().notifications.clone();
        let cards: Vec<AnyElement> = notes
            .iter()
            .rev()
            .map(|n| self.notification(theme, n, cx))
            .collect();
        let dashboard = self.dashboard.clone();
        // 通知が多いと収まらないので、中身は縦にスクロールする。
        components::panel(theme).child(
            div()
                .id("quick-settings")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(spacing::MD))
                // Android のクイック設定のように、上に時刻・日付と設定へのボタン。
                .child(
                    div()
                        .flex()
                        .items_center()
                        .px(px(spacing::SM))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .items_baseline()
                                .gap(px(spacing::MD))
                                .child(
                                    div()
                                        .debug_selector(|| "quick-time".into())
                                        .text_size(px(QUICK_TIME_PX))
                                        .child(fixed_clock(time_width)),
                                )
                                .child(
                                    div()
                                        .text_size(px(font_size::MD))
                                        .text_color(rgba(theme.neutral_text_low))
                                        .child(i18n::date(cx, now)),
                                ),
                        )
                        .child(
                            components::icon_button(
                                theme,
                                "quick-settings-open",
                                &icons::GEAR,
                                i18n::t(cx, "dashboard.settings"),
                                Variant::OnBackground,
                            )
                            // バーの設定ボタンと同じ（設定画面がこの画面と入れ替わりにはまる）。
                            .on_click(move |_, _, cx| {
                                let _ = dashboard.update(cx, |d, cx| d.toggle(Page::Settings, cx));
                            }),
                        ),
                )
                .child(section(theme, i18n::t(cx, "dashboard.notifications")))
                .child(if cards.is_empty() {
                    div()
                        .debug_selector(|| "notifications-empty".into())
                        .px(px(spacing::SM))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(i18n::t(cx, "dashboard.no_notifications"))
                } else {
                    div().flex().flex_col().gap(px(spacing::SM)).children(cards)
                }),
        )
    }

    /// 一覧の 1 枚。ホバー中は広げる（本文が伸び、通知のボタンが並ぶ）。押すと開く（URL は確かめてから、ほかはアプリへ）。
    fn notification(
        &mut self,
        theme: &Theme,
        n: &Arc<Notification>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = n.id;
        let image = self.notification_icons.of(n);
        let (panel, note) = (self.panel, n.clone());
        let card = ui::notification::card(
            theme,
            "notification",
            n,
            image,
            self.notification_hover == Some(id),
            move |action, window, cx| ui::notification::open(panel, &note, action, window, cx),
        );
        div()
            .id(SharedString::from(format!("notification-card-{id}")))
            .w_full()
            .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                let next = match (*on, this.notification_hover) {
                    (true, _) => Some(id),
                    (false, Some(h)) if h == id => None,
                    (false, h) => h,
                };
                if next != this.notification_hover {
                    this.notification_hover = next;
                    ui::notify(this.panel, cx);
                }
            }))
            .child(card)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
