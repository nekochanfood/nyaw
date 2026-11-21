//! クイック設定（通知センター）: 上段に時刻・日付と設定ボタン、その下に OS 通知の一覧（新しい順。カードは
//! `ui::notification::card` を使用）。詳細はここで確認する: ホバー（トーストから開いた場合はその通知）で展開し、
//! 本文・通知ボタン・「（アプリ）を開く」（配信して開く）が表示される（滑らかに伸縮する）。押下で開く。
//! 左右スワイプまたは「すべて消去」で削除する（OS 通知センターからも削除）。通知が多い場合は一覧のみ
//! スクロールし、時刻と見出しは固定。通知がなければ中央にチェックと「通知はありません」を表示する。

use super::*;
use ui::notification::{Center, Detail};
use ui::swipe::Look;

/// 「すべて消去」でカードが上から順に退出するときの間隔。件数が多い場合は合計を [`CLEAR_SPAN`] 以内に収める。
const CLEAR_STAGGER: Duration = Duration::from_millis(60);
const CLEAR_SPAN: Duration = Duration::from_millis(600);

fn clear_stagger(count: usize) -> Duration {
    CLEAR_STAGGER.min(CLEAR_SPAN / count.max(1) as u32)
}

/// 時刻・日付と見出しの、背景余白からさらに下げる幅（パネル角に近づきすぎないようにする）。
const QUICK_INSET: f32 = spacing::LG;
/// 時刻・日付行の行高（フォントサイズに対する倍率）。
const QUICK_LEADING: f32 = 1.2;

/// 時刻の横に下端揃えで配置した日付を、時刻のベースラインまで持ち上げるオフセット。GPUI にはベースライン揃え機能が
/// ないため、行ボックス下端からベースラインまでの高さの差をフォントメトリクスから算出する（テキストシステムと同じ
/// 組版・丸め方式。行高倍率も適用される）。
fn date_lift(window: &Window) -> gpui::Pixels {
    let below = |size: f32, weight: FontWeight| {
        let run = gpui::TextRun {
            len: 1,
            font: gpui::Font {
                weight,
                ..design::font::font()
            },
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line("0".into(), px(size), &[run], None);
        let height = design::font::scaled(size * QUICK_LEADING).round();
        (height - line.ascent + line.descent) / 2.0
    };
    below(QUICK_TIME_PX, FontWeight::BOLD) - below(font_size::MD, FontWeight::NORMAL)
}

/// 通知が無い場合に中央に表示するチェックアイコンの一辺。
const EMPTY_ICON_PX: f32 = 64.0;

/// この期間内は「n 日前」のように相対表記し、超過後は日付と時刻で表示する。
const RELATIVE_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 通知の受信時刻を表示用文字列に変換する。1 分以内は「今」、以降「n 分前」「n 時間前」「n 日前」、
/// [`RELATIVE_FOR`] を超過した場合は日付と時刻。
pub(crate) fn stamp(
    cx: &App,
    arrived: std::time::SystemTime,
    now: chrono::DateTime<chrono::Local>,
) -> SharedString {
    let at = chrono::DateTime::<chrono::Local>::from(arrived);
    // 時計のずれで未来の時刻になっていても「今」として扱う。
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
    /// トーストから通知センターを開いた際の処理: 一覧の表示元パネルを登録し、展開対象の通知を受け取る
    /// （以降の要求も受け付ける）。
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
        // スワイプで完全に折り畳まれた通知は OS 通知センターから削除する（一覧から除去されるまで非表示）。
        for id in self.swipes.take_gone(tick) {
            self.dismissed.insert(id);
            ui::request(Request::RemoveNotification(id), cx);
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
        // 「（アプリ）を開く」: ウィンドウ一覧の取得後に配信して開く（未起動の場合は起動を待つ）。
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
        // 表示中の一覧に含まれる通知を既読にする（ドッキング中かつダッシュボードが表示されている場合）。
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
                    // 上段に時刻・日付（日付は時刻のベースラインに揃える）と設定ボタンを配置する。
                    // パネル角から離し、左端は見出しと揃える。
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .px(px(QUICK_INSET))
                            .pt(px(spacing::SM))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_end()
                                    .gap(px(spacing::MD))
                                    .child(
                                        div()
                                            .debug_selector(|| "quick-time".into())
                                            .flex_none()
                                            // 数字による幅変動で日付が移動しないよう固定幅にする（左寄せ）。
                                            .w(time_width)
                                            .text_size(px(QUICK_TIME_PX))
                                            .line_height(gpui::relative(QUICK_LEADING))
                                            .font_weight(FontWeight::BOLD)
                                            .child(now.format("%H:%M").to_string()),
                                    )
                                    .child(
                                        div()
                                            .debug_selector(|| "quick-date".into())
                                            .min_w_0()
                                            .truncate()
                                            .pb(date_lift(window))
                                            .text_size(px(font_size::MD))
                                            .line_height(gpui::relative(QUICK_LEADING))
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
                                // バーの設定ボタンと同一動作（設定画面がこの画面と入れ替わりにドッキングする）。
                                .on_click(move |_, _, cx| {
                                    let _ =
                                        dashboard.update(cx, |d, cx| d.toggle(Page::Settings, cx));
                                }),
                            ),
                    )
                    .child(
                        section(theme, i18n::t(cx, "dashboard.notifications"))
                            .px(px(QUICK_INSET))
                            .when(!cards.is_empty(), |s| {
                                s.child(
                                    components::button(
                                        theme,
                                        "notifications-clear",
                                        i18n::t(cx, "dashboard.clear_notifications"),
                                        Variant::OnBackground,
                                    )
                                    // 上から順にスワイプと同様に横へ退出・折り畳みし、完了後 OS 通知センターから削除する。
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            let now = Instant::now();
                                            let gap = clear_stagger(ids.len());
                                            for (i, &id) in ids.iter().enumerate() {
                                                this.swipes.dismiss(id, now + gap * i as u32);
                                            }
                                            ui::notify(this.panel, cx);
                                        },
                                    )),
                                )
                            }),
                    )
                    .child(if cards.is_empty() {
                        // 空であることが一目で分かるよう、中央にチェックアイコンと「通知はありません」を表示する。
                        div()
                            .debug_selector(|| "notifications-empty".into())
                            .flex_1()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(spacing::MD))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(
                                design::icon(&icons::CHECK_CIRCLE)
                                    .size(px(EMPTY_ICON_PX))
                                    .color(rgba(theme.neutral_text_low)),
                            )
                            .child(i18n::t(cx, "dashboard.no_notifications"))
                            .into_any_element()
                    } else {
                        // 通知が多い場合に収まらないため、一覧のみ縦スクロールする（時刻と見出しは固定）。
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
            // カードのスワイプイベントを受け取る（カードより後に描画する）。
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| swipes.listen(panel, window),
                )
                .absolute()
                .size_full(),
            )
    }

    /// 一覧の 1 枚。ホバー中（トーストから開いた場合はその通知）は展開する（本文が伸び、通知ボタンと「（アプリ）を開く」が
    /// 表示される）。押下で開く（URL は検証後、それ以外はアプリへ）。スワイプで横に退出後、高さを折り畳み下を詰める。
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
        let note = n.clone();
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
            move |action, window, cx| ui::notification::open(&note, action, window, cx),
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
            // カード間の余白もカードと共に折り畳む（完全折り畳み時に余白分だけジャンプしないようにする）。
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
