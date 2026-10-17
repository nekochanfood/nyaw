//! Layer 6 画面: 常駐パネル「wrist」。左手首に腕時計のように取り付き（掴めない）、丸い文字盤に
//! 日付・時刻・Toggle Panel ボタンを出す（縁に目盛りと秒の点）。ボタンで召喚するパネル（main が選ぶ。今は Testing）を開く / 閉じる。
//! 常駐なので、X ボタンの二回連続押しでダッシュボードを開く / 閉じるのもここ。

use std::time::Duration;

use chrono::Timelike;
use design::components::{self, Variant};
use design::{Theme, font_size, icons, radius, spacing};
use gpui::{Context, FontWeight, Window, div, prelude::*, px, rgba};
use kernel::vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId};
use ui::{PanelControl, PanelSpec};

/// 手首の甲側に載せた腕時計の相対姿勢: コントローラ原点の少し手首寄り（+Z）・上（+Y）で、
/// 文字面が上を向き、天面が前を向く（手首を返すと読める）。
// ponytail: コントローラ座標系（-Z が前・+Y が上）の目分量。機種ごとの握りの差は offset の調整で合わせる。
// 骨格だけの手は、取り付け先の姿勢（`vr::HandSkeleton::grip`）の元をずらして、同じ値で腕時計の位置に来るようにしてある。
pub const WATCH_OFFSET: Pose = Pose::new(
    Vec3::new(0.0, 0.04, 0.08),
    Quat::from_xyzw(-0.707_106_77, 0.0, 0.0, 0.707_106_77),
);
/// 取り付け先の手が追えない間（起動直後・novr のエミュレーター）の置き場所。開いた瞬間の頭から見て左下 0.4m。
/// 召喚するパネル（[`SUMMON_POSE`]）とは視線の方向が重ならない。
pub const IN_VIEW: Pose = Pose::new(Vec3::new(-0.32, -0.2, -0.4), Quat::IDENTITY);
/// 召喚するパネルの位置。召喚した瞬間の頭から見て正面 0.8m、少し下。
pub const SUMMON_POSE: Pose = Pose::new(Vec3::new(0.0, -0.1, -0.8), Quat::IDENTITY);

/// 丸い文字盤（直径 0.1m）と、周りの影の余白（[`components::PANEL_MARGIN`]）が収まる正方形。
/// 4px/mm（旧来の 0.12m / 480px と同じ密度）。
const SIZE_M: Vec2 = Vec2::new(0.112, 0.112);
const RES_PX: (u32, u32) = (448, 448);
/// 文字盤（カード）の中での中心。
const DIAL_C: f32 = RES_PX.0 as f32 / 2.0 - components::PANEL_MARGIN;
/// 文字盤の中心から目盛り（と秒の点）の中心までの半径。縁から `spacing::LG` 内側。
const DIAL_R: f32 = DIAL_C - spacing::LG - 8.0;

/// Toggle Panel で召喚するパネルの起動仕様を作る。開いたら引数で PanelId を知らせる（`dashboard::spec` と同じ形）。
/// 画面どうしを直接つながないよう、何を召喚するかは main が渡す。
pub type Summon = fn(Box<dyn FnOnce(PanelId, &mut gpui::App)>) -> PanelSpec;

/// 左手首に取り付いた状態で起動する wrist の起動仕様。
pub fn spec(summon: Summon) -> PanelSpec {
    PanelSpec::new(Pose::IDENTITY, move |panel, _, cx| {
        Wrist::new(panel, summon, cx)
    })
    .sized(SIZE_M, RES_PX)
    .in_view(IN_VIEW)
    .attached(Device::Controller(Hand::Left), WATCH_OFFSET)
    .locked()
    // 手首に付いた常駐の時計で、閉じる・動かすものではない。ナビゲーションバーは付けない。
    .without_nav_bar()
}

/// 今の言語での日付（書式は文言の `wrist.date`。chrono の書式で、曜日だけを `{weekday}` で訳す）。
fn date(now: chrono::DateTime<chrono::Local>, cx: &gpui::App) -> String {
    use chrono::Datelike;
    let weekday = match now.weekday() {
        chrono::Weekday::Sun => i18n::t(cx, "weekday.sun"),
        chrono::Weekday::Mon => i18n::t(cx, "weekday.mon"),
        chrono::Weekday::Tue => i18n::t(cx, "weekday.tue"),
        chrono::Weekday::Wed => i18n::t(cx, "weekday.wed"),
        chrono::Weekday::Thu => i18n::t(cx, "weekday.thu"),
        chrono::Weekday::Fri => i18n::t(cx, "weekday.fri"),
        chrono::Weekday::Sat => i18n::t(cx, "weekday.sat"),
    };
    let format = i18n::tf(cx, "wrist.date", &[("weekday", &weekday)]);
    now.format(&format).to_string()
}

pub struct Wrist {
    /// 召喚中のパネル。
    pub summoned: Option<PanelId>,
    summon: Summon,
    /// 開いているダッシュボード。
    pub dashboard: Option<PanelId>,
    /// このパネル。
    panel: PanelId,
    /// 反映した X ボタンの二回押しの回数（[`kernel::PanelStatus::double_press`]）。
    toggles: u32,
}

impl Wrist {
    pub fn new(panel: PanelId, summon: Summon, cx: &mut Context<Self>) -> Self {
        // 時計を進める: 1 秒ごとに描き直す（フレームループはビューの notify を知らないので `redraw` で頼む）。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let redraw = |_: &mut Self, cx: &mut Context<Self>| {
                    cx.notify();
                    cx.default_global::<PanelControl>()
                        .panels
                        .entry(panel)
                        .or_default()
                        .redraw = true;
                };
                if this.update(cx, redraw).is_err() {
                    return;
                }
            }
        })
        .detach();
        cx.observe_global::<PanelControl>(Self::sync_dashboard)
            .detach();
        Self {
            summoned: None,
            summon,
            dashboard: None,
            panel,
            toggles: 0,
        }
    }

    /// X ボタンを二回押すたびにダッシュボードを開く・閉じる（音はダッシュボード自身が鳴らす）。
    /// フレームループが状態を書き戻すたびに呼ばれるので、変わっていなければ何もしない（書き込むと自分がまた呼ばれる）。
    fn sync_dashboard(&mut self, cx: &mut Context<Self>) {
        let ctl = cx.global::<PanelControl>();
        let toggles = ctl
            .panels
            .get(&self.panel)
            .map_or(0, |p| p.status.double_press);
        if toggles == self.toggles {
            return;
        }
        self.toggles = toggles;
        // ナビゲーションバーで閉じられていたら、閉じ済みとして扱い次は開く。
        let open = self
            .dashboard
            .take()
            .filter(|id| !ctl.panels.get(id).is_some_and(|p| p.status.closed));
        match open {
            Some(id) => cx
                .default_global::<PanelControl>()
                .panels
                .entry(id)
                .or_default()
                .commands
                .push(PanelCommand::Close),
            None => {
                let wrist = cx.entity();
                cx.default_global::<PanelControl>()
                    .spawns
                    .push(dashboard::spec(move |id, cx| {
                        wrist.update(cx, |w, _| w.dashboard = Some(id));
                    }));
            }
        }
    }
}

impl Render for Wrist {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = chrono::Local::now();
        let this = cx.entity();
        let theme = Theme::current(cx);
        // 腕時計の文字盤: 丸いカード 1 枚がパネル全体（地は敷かない）。縁に 12 の目盛りと、今の秒を指す点。
        let dot = |i: u32, d: f32, color: u32| {
            let a = i as f32 * std::f32::consts::TAU / 60.0;
            div()
                .absolute()
                .left(px(DIAL_C + DIAL_R * a.sin() - d / 2.0))
                .top(px(DIAL_C - DIAL_R * a.cos() - d / 2.0))
                .size(px(d))
                .rounded(px(radius::FULL))
                .bg(rgba(color))
        };
        let ticks = (0..60).step_by(5).map(|i| {
            dot(
                i,
                if i % 15 == 0 { 14.0 } else { 8.0 },
                theme.neutral_text_low,
            )
        });
        let second = dot(now.second(), 12.0, theme.accent_solid);
        components::card_panel(&theme)
            .rounded(px(radius::FULL))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .children(ticks)
            .child(second)
            .child(
                div()
                    .id("date")
                    .debug_selector(|| "date".into())
                    .text_size(px(font_size::LG))
                    .text_color(rgba(theme.neutral_text_low))
                    .child(date(now, cx)),
            )
            .child(
                div()
                    .id("time")
                    .debug_selector(|| "time".into())
                    .text_size(px(96.0))
                    .font_weight(FontWeight::BOLD)
                    .line_height(px(104.0))
                    .child(now.format("%H:%M").to_string()),
            )
            .child(
                components::icon_button(
                    &theme,
                    "toggle-panel",
                    &icons::APP_WINDOW,
                    i18n::t(cx, "wrist.toggle_panel"),
                    Variant::Default,
                )
                .mt(px(spacing::SM))
                .on_click(move |_, _, cx| {
                    let this = this.clone();
                    this.update(cx, |w, cx| {
                        // 召喚したパネル自身のボタンで閉じられていたら、閉じ済みとして扱い次は召喚する。
                        let open = w.summoned.take().filter(|id| {
                            !cx.default_global::<PanelControl>()
                                .panels
                                .get(id)
                                .is_some_and(|p| p.status.closed)
                        });
                        match open {
                            Some(id) => cx
                                .default_global::<PanelControl>()
                                .panels
                                .entry(id)
                                .or_default()
                                .commands
                                .push(kernel::PanelCommand::Close),
                            None => {
                                let wrist = cx.entity();
                                let spec = (w.summon)(Box::new(move |id, cx| {
                                    wrist.update(cx, |w, _| w.summoned = Some(id));
                                }))
                                .in_view(SUMMON_POSE);
                                cx.default_global::<PanelControl>().spawns.push(spec);
                            }
                        }
                    });
                }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};

    /// ボタンは 召喚の要求を積み、もう一度押すと閉じる要求を積む。
    #[gpui::test]
    fn toggle_panel_spawns_then_closes_home(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| {
            Wrist::new(
                0,
                |_| PanelSpec::new(Pose::IDENTITY, |_, _, _| gpui::Empty),
                cx,
            )
        });
        cx.run_until_parked();
        let c = cx.debug_bounds("toggle-panel").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            1
        );
        wrist.update(cx, |w, _| w.summoned = Some(3));
        cx.simulate_click(c, Modifiers::none());
        let q = cx.update(|_, cx| cx.global::<PanelControl>().panels[&3].commands.clone());
        assert_eq!(q, [kernel::PanelCommand::Close]);
    }

    /// 召喚したパネルが自分のボタンで閉じられた後は、トグルでもう一度召喚される。
    #[gpui::test]
    fn toggle_summons_again_after_home_closed_itself(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| {
            Wrist::new(
                0,
                |_| PanelSpec::new(Pose::IDENTITY, |_, _, _| gpui::Empty),
                cx,
            )
        });
        cx.run_until_parked();
        wrist.update(cx, |w, _| w.summoned = Some(3));
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(3)
                .or_default()
                .status
                .closed = true
        });
        let c = cx.debug_bounds("toggle-panel").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            1
        );
    }

    /// X ボタンを二回押すとダッシュボードを開き、もう一度二回押すと閉じる。
    #[gpui::test]
    fn double_x_opens_then_closes_dashboard(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| {
            Wrist::new(
                0,
                |_| PanelSpec::new(Pose::IDENTITY, |_, _, _| gpui::Empty),
                cx,
            )
        });
        cx.run_until_parked();
        let press_twice = |cx: &mut gpui::VisualTestContext, n| {
            cx.update(|_, cx| {
                cx.default_global::<PanelControl>()
                    .panels
                    .entry(0)
                    .or_default()
                    .status
                    .double_press = n
            });
            cx.run_until_parked();
        };
        let take = |cx: &mut gpui::VisualTestContext, id| {
            cx.update(|_, cx| {
                std::mem::take(
                    &mut cx
                        .default_global::<PanelControl>()
                        .panels
                        .entry(id)
                        .or_default()
                        .commands,
                )
            })
        };
        press_twice(cx, 1);
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            1
        );
        wrist.update(cx, |w, _| w.dashboard = Some(3));
        press_twice(cx, 2);
        assert_eq!(take(cx, 3), [PanelCommand::Close]);
        assert!(take(cx, 0).is_empty(), "音は手首からは鳴らさない");
        assert_eq!(wrist.read_with(cx, |w, _| w.dashboard), None);
    }

    /// 丸い文字盤の中央に、日付・時刻・ボタンを縦に並べる。
    #[gpui::test]
    fn layout_is_date_over_time_over_button(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| {
            Wrist::new(
                0,
                |_| PanelSpec::new(Pose::IDENTITY, |_, _, _| gpui::Empty),
                cx,
            )
        });
        cx.run_until_parked();
        let [d, t, b] = ["date", "time", "toggle-panel"].map(|s| cx.debug_bounds(s).unwrap());
        assert!(d.bottom() <= t.top());
        assert!(t.bottom() <= b.top());
        let mid = |r: gpui::Bounds<gpui::Pixels>| r.center().x;
        assert!((mid(d) - mid(t)).abs() < px(1.0) && (mid(t) - mid(b)).abs() < px(1.0));
    }
}
