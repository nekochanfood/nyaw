//! クイック設定（通知センター）: 上に時刻・日付と設定ボタン、その下に届いた OS の通知の一覧（新しい順。カードは通知の
//! トーストと同じ `ui::notification::card`）。詳しい中身はここで見る: ホバー（トーストから開いたときはその通知）で広げ、
//! 本文が伸びて通知のボタンと、アプリを開けるなら「（アプリ）を開く」（映して開く）が並ぶ。押すと開く。左右にスワイプ
//! すると片付く（OS の通知センターからも消す）。

use super::*;
use ui::notification::Center;
use ui::swipe::Look;

impl Screen {
    /// トーストから通知センターを開いた: 一覧を出しているのはこのパネルだと知らせ、広げて見せる通知を受け取る
    /// （以降の頼みも受ける）。
    pub(crate) fn answer_center(&mut self, panel: PanelId, cx: &mut Context<Self>) {
        let center = cx.default_global::<Center>();
        center.panel = Some(panel);
        self.notification_focus = center.focus.take();
        cx.observe_global::<Center>(|this, cx| {
            if cx.global::<Center>().focus.is_none() {
                return;
            }
            this.notification_focus = cx.default_global::<Center>().focus.take();
            this.notification_hover = None;
            ui::notify(this.panel, cx);
        })
        .detach();
    }

    pub(crate) fn quick_settings(
        &mut self,
        theme: &Theme,
        time_width: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let now = chrono::Local::now();
        let tick = Instant::now();
        // スワイプで畳みきった通知は、OS の通知センターから消す（一覧から落ちるまでは並べない）。
        for id in self.swipes.take_gone(tick) {
            self.dismissed.insert(id);
            ui::command(self.panel, PanelCommand::RemoveNotification(id), cx);
        }
        let notes = cx.global::<PanelControl>().notifications.clone();
        self.dismissed
            .retain(|id| notes.iter().any(|n| n.id == *id));
        let shown: Vec<Arc<Notification>> = notes
            .iter()
            .rev()
            .filter(|n| !self.dismissed.contains(&n.id))
            .cloned()
            .collect();
        let cards: Vec<AnyElement> = shown
            .iter()
            .map(|n| self.notification(theme, n, tick, cx))
            .collect();
        self.swipes
            .forget(&shown.iter().map(|n| n.id).collect::<Vec<_>>());
        // 「（アプリ）を開く」: ウィンドウの一覧が届いたら映して開く（起動していなければ起動して待つ）。
        self.poll_sources();
        if self.pending.is_none()
            && let Some(app) = self.open_after.take()
        {
            self.launch_app(app, cx);
        }
        if self.swipes.busy() || self.open_after.is_some() {
            components::keep_drawing(window, cx);
        }
        let dashboard = self.dashboard.clone();
        let (swipes, panel) = (self.swipes.clone(), self.panel);
        // 通知が多いと収まらないので、中身は縦にスクロールする。
        // 出ている一覧に並んだものは読んだことにする（はまっていて、ダッシュボードが出ているとき）。
        if ui::docked(cx) == Some(self.panel) {
            ui::notification::mark_read(shown.iter().map(|n| n.id), cx);
        }
        components::panel(theme)
            .child(
                div()
                    .id("quick-settings")
                    .debug_selector(|| "quick-settings".into())
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
                                    let _ =
                                        dashboard.update(cx, |d, cx| d.toggle(Page::Settings, cx));
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
                        div().flex().flex_col().children(cards)
                    }),
            )
            // カードのスワイプを受ける（カードより後に描く）。
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| swipes.listen(panel, window),
                )
                .absolute()
                .size_full(),
            )
    }

    /// 一覧の 1 枚。ホバー中（トーストから開いたときはその通知）は広げる（本文が伸び、通知のボタンと「（アプリ）を開く」が
    /// 並ぶ）。押すと開く（URL は確かめてから、ほかはアプリへ）。スワイプで横へ抜けたら、高さを畳んで下を詰める。
    fn notification(
        &mut self,
        theme: &Theme,
        n: &Arc<Notification>,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = n.id;
        let image = self.notification_icons.of(n);
        let expanded = match self.notification_hover {
            Some(h) => h == id,
            None => self.notification_focus == Some(id),
        };
        let open = expanded
            .then(|| self.open_app_button(theme, n, cx))
            .flatten();
        let (panel, note) = (self.panel, n.clone());
        let card = ui::notification::card(
            theme,
            "notification",
            n,
            image,
            expanded,
            open,
            move |action, window, cx| ui::notification::open(panel, &note, action, window, cx),
        );
        let look: Look = self.swipes.look(id, now);
        let natural = self
            .swipes
            .bounds(id)
            .map_or(0.0, |b| f32::from(b.size.height));
        div()
            .id(SharedString::from(format!("notification-card-{id}")))
            .w_full()
            .flex_none()
            // カードの隙間もカードと一緒に畳む（畳みきったときに隙間の分だけ跳ばないように）。
            .pb(px(spacing::SM))
            .when(look.height < 1.0, |d| {
                d.h(px((natural + spacing::SM) * look.height))
                    .overflow_hidden()
            })
            .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                let next = match (*on, this.notification_hover) {
                    (true, _) => Some(id),
                    (false, Some(h)) if h == id => None,
                    (false, h) => h,
                };
                if next != this.notification_hover {
                    // トーストから開いて広げていた通知は、ほかを指したらたたむ。
                    if *on {
                        this.notification_focus = None;
                    }
                    this.notification_hover = next;
                    ui::notify(this.panel, cx);
                }
            }))
            .child(self.swipes.item(id, look, card))
            .into_any_element()
    }

    /// 通知のアプリを開くボタン（`notification-open-<通知の ID>`）。入っているアプリ（ランチャーの一覧）にあるときだけ。
    /// 押すとそのウィンドウを映して開く（起動していなければ起動して待つ。ランチャーと同じ）。
    fn open_app_button(
        &self,
        theme: &Theme,
        n: &Notification,
        cx: &mut Context<Self>,
    ) -> Option<components::Button> {
        let app = cx
            .try_global::<AppCatalog>()?
            .0
            .iter()
            .find(|(a, _)| a.id.eq_ignore_ascii_case(&n.app.id))
            .map(|(a, _)| a.clone())?;
        let label = i18n::tf(cx, "dashboard.open_notification_app", &[("app", &app.name)]);
        Some(
            components::button(
                theme,
                format!("notification-open-{}", n.id),
                label,
                Variant::Default,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                // カードの本文を押したことにしない。
                cx.stop_propagation();
                // 起動しているかを今のウィンドウの一覧で見るため、取り直してから開く。
                this.refresh();
                this.open_after = Some(app.clone());
                ui::notify(this.panel, cx);
            })),
        )
    }
}

#[cfg(test)]
mod tests;
