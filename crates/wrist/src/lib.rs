//! Layer 6 画面: 常駐パネル「wrist」。左手首に腕時計のように取り付き（掴めない）、"時刻\n日付" と
//! Toggle Panel ボタンを出す。ボタンで Testing パネルを召喚 / 閉じる。

use std::time::Duration;

use design::components::{Variant, button};
use design::{Mode, Theme};
use gpui::{Context, Window, div, prelude::*, rgba};
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

const SIZE_M: Vec2 = Vec2::new(0.12, 0.06);
const RES_PX: (u32, u32) = (480, 240);

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
        let theme = Theme::of(Mode::Dark);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgba(theme.panel))
            .text_color(rgba(theme.neutral_text))
            .text_size(gpui::px(48.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("time")
                            .debug_selector(|| "time".into())
                            .child(now.format("%H:%M").to_string()),
                    )
                    .child(
                        div()
                            .id("date")
                            .debug_selector(|| "date".into())
                            .child(now.format("%Y-%m-%d").to_string()),
                    ),
            )
            .child(
                button(&theme, "toggle-panel", "Toggle Panel", Variant::Primary)
                    .text_size(gpui::px(32.0))
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

    /// 時刻の下に日付、その右隣にボタン。
    #[gpui::test]
    fn layout_is_time_over_date_with_button_on_right(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| Wrist::new(cx));
        cx.run_until_parked();
        let [t, d, b] = ["time", "date", "toggle-panel"].map(|s| cx.debug_bounds(s).unwrap());
        assert!(t.bottom() <= d.top());
        assert!(b.left() >= t.right().max(d.right()));
    }
}
