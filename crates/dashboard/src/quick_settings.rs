//! クイック設定（通知センター）: 上に時刻・日付と設定ボタン、その下に届いた OS の通知の一覧（新しい順。カードは通知の
//! トーストと同じ `ui::notification::card`）。詳しい中身はここで見る: ホバー（トーストから開いたときはその通知）で広げ、
//! 本文が伸びて通知のボタンと、アプリを開けるなら「（アプリ）を開く」（映して開く）が並ぶ（滑らかに伸び縮みする）。押すと
//! 開く。左右にスワイプするか「すべて消去」で片付く（OS の通知センターからも消す）。通知が多いときは一覧だけが
//! スクロールし、時刻と見出しは動かない。

use super::*;
use ui::notification::{Center, Detail};
use ui::swipe::Look;

/// 届いてからこの間は「n 日前」のように今からの差で、過ぎたら日付と時刻で出す。
const RELATIVE_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 通知が届いた時刻の出し方: 1 分以内は「今」、それから「n 分前」「n 時間前」「n 日前」、[`RELATIVE_FOR`] より古ければ
/// 日付と時刻。
pub(crate) fn stamp(
    cx: &App,
    arrived: std::time::SystemTime,
    now: chrono::DateTime<chrono::Local>,
) -> SharedString {
    let at = chrono::DateTime::<chrono::Local>::from(arrived);
    // 時計がずれて先の時刻になっていても「今」。
    let ago = (now - at).to_std().unwrap_or_default().as_secs();
    let (minute, hour, day) = (60, 60 * 60, 24 * 60 * 60);
    let ago_in =
        |key, unit: u64| i18n::tf(cx, key, &[("n", &(ago / unit) as &dyn std::fmt::Display)]);
    if ago < minute {
        i18n::t(cx, "dashboard.notification_now")
    } else if ago < hour {
        ago_in("dashboard.notification_minutes_ago", minute)
    } else if ago < day {
        ago_in("dashboard.notification_hours_ago", hour)
    } else if ago < RELATIVE_FOR.as_secs() {
        ago_in("dashboard.notification_days_ago", day)
    } else {
        at.format(&i18n::t(cx, "dashboard.notification_date"))
            .to_string()
            .into()
    }
}

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
        let ids: Vec<u32> = shown.iter().map(|n| n.id).collect();
        self.swipes.forget(&ids);
        self.expanding.forget(&ids);
        // 「（アプリ）を開く」: ウィンドウの一覧が届いたら映して開く（起動していなければ起動して待つ）。
        self.poll_sources();
        if self.pending.is_none()
            && let Some(app) = self.open_after.take()
        {
            self.launch_app(app, cx);
        }
        if self.swipes.busy() || self.expanding.busy(tick) || self.open_after.is_some() {
            components::keep_drawing(window, cx);
        }
        let dashboard = self.dashboard.clone();
        let (swipes, panel) = (self.swipes.clone(), self.panel);
        // 出ている一覧に並んだものは読んだことにする（はまっていて、ダッシュボードが出ているとき）。
        if ui::docked(cx) == Some(self.panel) {
            ui::notification::mark_read(ids.iter().copied(), cx);
        }
        components::panel(theme)
            .child(
                div()
                    .debug_selector(|| "quick-settings".into())
                    .size_full()
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
                    .child(section(theme, i18n::t(cx, "dashboard.notifications")).when(
                        !cards.is_empty(),
                        |s| {
                            s.child(
                                components::button(
                                    theme,
                                    "notifications-clear",
                                    i18n::t(cx, "dashboard.clear_notifications"),
                                    Variant::OnBackground,
                                )
                                // その場で薄れながら畳まれ、畳みきったら OS の通知センターから消す（スワイプと同じ）。
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let now = Instant::now();
                                        for &id in &ids {
                                            this.swipes.dismiss(id, now);
                                        }
                                        ui::notify(this.panel, cx);
                                    },
                                )),
                            )
                        },
                    ))
                    .child(if cards.is_empty() {
                        div()
                            .debug_selector(|| "notifications-empty".into())
                            .px(px(spacing::SM))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::t(cx, "dashboard.no_notifications"))
                            .into_any_element()
                    } else {
                        // 通知が多いと収まらないので、一覧だけが縦にスクロールする（時刻と見出しは動かない）。
                        div()
                            .id("notifications-list")
                            .debug_selector(|| "notifications-list".into())
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(cards)
                            .into_any_element()
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
        let expand = self
            .expanding
            .look(id, expanded, now, cx.has_global::<design::Motion>());
        let open = expand
            .open
            .then(|| self.open_app_button(theme, n, cx))
            .flatten();
        let (panel, note) = (self.panel, n.clone());
        let detail = Detail {
            expand,
            stamp: Some(stamp(cx, n.arrived, chrono::Local::now())),
            extra: open.into_iter().collect(),
        };
        let card = ui::notification::card(
            theme,
            "notification",
            n,
            image,
            detail,
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
                // 画素に揃える（端数で動くと、下のカードの文字だけが画素に吸い付いて震える）。
                d.h(px(((natural + spacing::SM) * look.height).round()))
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
