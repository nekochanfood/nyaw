//! Layer 6 画面: 常駐パネル「wrist」。左手首に腕時計のように取り付き（掴めない）、丸い文字盤に
//! 日付・時刻・Toggle Panel ボタンを出す（縁に目盛りと秒の点）。ボタンで Testing パネルを召喚 / 閉じる。

use std::time::Duration;

use chrono::Timelike;
use design::components::{self, Variant};
use design::{Theme, font_size, icons, radius, spacing};
use gpui::{Context, FontWeight, Window, div, prelude::*, px, rgba};
use kernel::PanelId;
use kernel::vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
use testing::Testing;
use ui::{PanelControl, PanelSpec};

/// 手首の甲側に載せた腕時計の相対姿勢: コントローラ原点の少し手首寄り（+Z）・上（+Y）で、
/// 文字面が上を向き、天面が前を向く（手首を返すと読める）。
// ponytail: コントローラ座標系（-Z が前・+Y が上）の目分量。機種ごとの握りの差は offset の調整で合わせる。
pub const WATCH_OFFSET: Pose = Pose::new(
    Vec3::new(0.0, 0.04, 0.08),
    Quat::from_xyzw(-0.707_106_77, 0.0, 0.0, 0.707_106_77),
);
/// 取り付け先の手が追えない間（起動直後・novr のエミュレーター）の置き場所。開いた瞬間の頭から見て左下 0.4m。
/// 召喚する Testing パネル（[`TESTING_POSE`]）とは視線の方向が重ならない。
pub const IN_VIEW: Pose = Pose::new(Vec3::new(-0.32, -0.2, -0.4), Quat::IDENTITY);
/// 召喚する Testing パネルの位置。召喚した瞬間の頭から見て正面 0.8m、少し下。
pub const TESTING_POSE: Pose = Pose::new(Vec3::new(0.0, -0.1, -0.8), Quat::IDENTITY);

/// 丸い文字盤（直径 0.1m）と、周りの影の余白（[`components::PANEL_MARGIN`]）が収まる正方形。
/// 4px/mm（旧来の 0.12m / 480px と同じ密度）。
const SIZE_M: Vec2 = Vec2::new(0.112, 0.112);
const RES_PX: (u32, u32) = (448, 448);
/// 文字盤（カード）の中での中心。
const DIAL_C: f32 = RES_PX.0 as f32 / 2.0 - components::PANEL_MARGIN;
/// 文字盤の中心から目盛り（と秒の点）の中心までの半径。縁から `spacing::LG` 内側。
const DIAL_R: f32 = DIAL_C - spacing::LG - 8.0;

/// 左手首に取り付いた状態で起動する wrist の起動仕様。
pub fn spec() -> PanelSpec {
    PanelSpec::new(Pose::IDENTITY, |_, _, cx| Wrist::new(cx))
        .sized(SIZE_M, RES_PX)
        .in_view(IN_VIEW)
        .attached(Device::Controller(Hand::Left), WATCH_OFFSET)
        .locked()
}

pub struct Wrist {
    /// 召喚中の Testing パネル。
    pub testing: Option<PanelId>,
}

impl Wrist {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 時計を進める: 1 秒ごとに描き直す。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        })
        .detach();
        Self { testing: None }
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
                    .child(now.format("%a %b %-d").to_string()),
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
                    "Toggle Panel",
                    Variant::Default,
                )
                .mt(px(spacing::SM))
                .on_click(move |_, _, cx| {
                    let this = this.clone();
                    this.update(cx, |w, cx| {
                        // Testing 自身のボタンで閉じられていたら、閉じ済みとして扱い次は召喚する。
                        let open = w.testing.take().filter(|id| {
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
                                cx.default_global::<PanelControl>().spawns.push(
                                    PanelSpec::new(Pose::IDENTITY, move |id, _, cx| {
                                        wrist.update(cx, |w, _| w.testing = Some(id));
                                        Testing::new(id)
                                    })
                                    .sized(testing::TESTING_SIZE, testing::TESTING_PX)
                                    .in_view(TESTING_POSE),
                                );
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

    /// ボタンは Testing の召喚要求を積み、もう一度押すと閉じる要求を積む。
    #[gpui::test]
    fn toggle_panel_spawns_then_closes_home(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| Wrist::new(cx));
        cx.run_until_parked();
        let c = cx.debug_bounds("toggle-panel").unwrap().center();
        cx.simulate_click(c, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            1
        );
        wrist.update(cx, |w, _| w.testing = Some(3));
        cx.simulate_click(c, Modifiers::none());
        let q = cx.update(|_, cx| cx.global::<PanelControl>().panels[&3].commands.clone());
        assert_eq!(q, [kernel::PanelCommand::Close]);
    }

    /// Testing が自分のボタンで閉じられた後は、トグルでもう一度召喚される。
    #[gpui::test]
    fn toggle_summons_again_after_home_closed_itself(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| Wrist::new(cx));
        cx.run_until_parked();
        wrist.update(cx, |w, _| w.testing = Some(3));
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

    /// 丸い文字盤の中央に、日付・時刻・ボタンを縦に並べる。
    #[gpui::test]
    fn layout_is_date_over_time_over_button(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| Wrist::new(cx));
        cx.run_until_parked();
        let [d, t, b] = ["date", "time", "toggle-panel"].map(|s| cx.debug_bounds(s).unwrap());
        assert!(d.bottom() <= t.top());
        assert!(t.bottom() <= b.top());
        let mid = |r: gpui::Bounds<gpui::Pixels>| r.center().x;
        assert!((mid(d) - mid(t)).abs() < px(1.0) && (mid(t) - mid(b)).abs() < px(1.0));
    }
}
