//! Layer 6 画面: 常駐パネル「wrist」。利き手の反対（既定は左）の手首に腕時計のようにアタッチする（ダッシュボード表示中のみ
//! グラブして、手から [`REACH`] 以内に再配置・リサイズ可能。リリースすると設定に保存し、次回起動でも同一位置・同一サイズでアタッチする。
//! グラブしたままダッシュボードが非表示になれば元に戻る）。丸い文字盤に日付・時刻を表示する（デザインは設定の `wrist_design`。[`face`]）。
//! 利き手の設定が変更されれば、自動で反対の手首へ再アタッチする。
//! 常駐パネルのため、X ボタンの二回連続押しでダッシュボードを表示 / 非表示にする処理もここで行う（ダッシュボードは閉じずに非表示にし、次回も同じ状態で表示する）。
//! OS 通知のトースト（[`toast`]）も起動時に開く。

mod face;
pub use face::preview;

use std::f32::consts::FRAC_1_SQRT_2;
use std::time::Duration;

use design::components;
use design::{Theme, radius};
use gpui::{Context, Window, prelude::*, px};
use kernel::vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId, Setting, Settings};
use ui::{PanelControl, PanelSpec};

/// `hand` の手首に巻いた腕時計の相対姿勢（コントローラの tip の座標系。-Z が前・+Y が天面（親指の側））。
/// 手首の関節（tip から [`kernel::vr::GRIP_FROM_WRIST`] の逆）から腕側へ 3cm・手の甲の側（握った手の外側。左手は -X、
/// 右手は +X）へ [`LIFT`]。文字盤の表は手の甲の外、12 時は小指の側を向き、3 時は左手なら指先・右手なら肘の側（本物の
/// 腕時計と同じ）。腕を胸の前で横にして手の甲を顔へ向けると正立して読める。骨格だけの手も、取り付け先の姿勢
/// （`HandSkeleton::grip`）が握ったコントローラの tip に当たるので、同じ値で手首に来る。
pub fn watch_offset(hand: Hand) -> Pose {
    let side = if hand == Hand::Left { -1.0 } else { 1.0 };
    Pose::new(
        -kernel::vr::GRIP_FROM_WRIST + Vec3::new(side * LIFT, 0.0, 0.03),
        // 左手は (1, 0, -1) 軸の半回転（右手はその鏡写し）: 3 時 → -Z、12 時 → -Y、表 → -X。
        Quat::from_xyzw(FRAC_1_SQRT_2, 0.0, side * FRAC_1_SQRT_2, 0.0),
    )
}

/// `hand` の手首にアタッチする姿勢: グラブして再配置した姿勢（設定の `wrist_offset`。左手首での値で、右手首にはミラーリング）があれば
/// それ、なければ [`watch_offset`]。
pub fn offset(hand: Hand, settings: &Settings) -> Pose {
    settings
        .wrist_offset
        .map_or_else(|| watch_offset(hand), |o| mirror(o, hand))
}

/// 左手首での姿勢を `hand` の手首での姿勢にする（右手首は X を裏返した鏡写し。[`watch_offset`] の左右と同じ）。
/// 鏡写しは 2 回で元に戻るので、`hand` の手首での姿勢を左手首での値に直すのにも使う。
fn mirror(pose: Pose, hand: Hand) -> Pose {
    if hand == Hand::Left {
        return pose;
    }
    let (p, q) = (pose.position, pose.orientation);
    Pose::new(
        Vec3::new(-p.x, p.y, p.z),
        Quat::from_xyzw(q.x, -q.y, -q.z, q.w),
    )
}

/// 手首関節から文字盤までの高さ（手の甲側。m）。実物の腕時計なら 3.5cm 程度だが、手の甲側まで回る
/// コントローラ（Index のストラップなど）の描画に文字盤がめり込むため、そこから 2cm 浮かせて 5.5cm。
// ponytail: 機種を問わない目分量。めり込む機種や浮きすぎて見える機種があれば、この値（か機種ごとの値）で合わせる。
const LIFT: f32 = 0.055;

/// ダッシュボード表示中にグラブして再配置可能な範囲（アタッチ先の手のコントローラから m）。手の甲・前腕・手の周囲に到達する。
pub const REACH: f32 = 0.3;

/// アタッチ先の手をトラッキングできない間（起動直後・novr のエミュレーター）の配置位置。表示時の頭部から見て左下 0.4m
/// （右手首にアタッチする場合は右下）。
pub const IN_VIEW: Pose = Pose::new(Vec3::new(-0.32, -0.2, -0.4), Quat::IDENTITY);

/// 丸い文字盤（直径 0.1m）と、周りの影の余白（[`components::PANEL_MARGIN`]）が収まる正方形。
/// 4px/mm（旧来の 0.12m / 480px と同じ密度）。
const SIZE_M: Vec2 = Vec2::new(0.112, 0.112);
const RES_PX: (u32, u32) = (448, 448);
// 文字盤（カード）の直径は、文字盤の寸法を決めた基準と同じ。
const _: () = assert!(RES_PX.0 as f32 - 2.0 * components::PANEL_MARGIN == face::BASE);

/// 現在の設定 `saved` の利き手の反対の手首に、保存済み姿勢とサイズ（[`offset`]・`wrist_scale`）でアタッチした状態で起動する
/// wrist の起動仕様。`debug` と `settings` はダッシュボードから開くデバッグ画面と設定画面（画面間を直接結合しないよう、
/// main が選択する）。
pub fn spec(saved: &Settings, debug: dashboard::Summon, settings: dashboard::Summon) -> PanelSpec {
    let hand = saved.dominant_hand.off_hand();
    let side = if hand == Hand::Left { 1.0 } else { -1.0 };
    let in_view = Pose::new(
        IN_VIEW.position * Vec3::new(side, 1.0, 1.0),
        IN_VIEW.orientation,
    );
    PanelSpec::root(Pose::IDENTITY, move |panel, _, cx| {
        Wrist::new(panel, debug, settings, cx)
    })
    .sized(SIZE_M * saved.wrist_scale, RES_PX)
    .in_view(in_view)
    .attached(Device::Controller(hand), offset(hand, saved))
    // グラブ可能なのはダッシュボード表示中のみ（[`Wrist::sync_dashboard`]）。再配置した位置とサイズはリリース時に保存する
    // （[`Wrist::remember_placement`]）。
    .locked()
    .reach(REACH)
    // 手首にアタッチされた常駐時計であり、閉じる対象ではない。ナビゲーションバーは付与しない。
    .without_nav_bar()
}

pub struct Wrist {
    debug: dashboard::Summon,
    settings: dashboard::Summon,
    /// ダッシュボード（一度開いたら閉じずに非表示にする）。
    pub dashboard: Option<PanelId>,
    /// OS の通知のトースト。
    pub toast: Option<PanelId>,
    /// このパネル。
    panel: PanelId,
    /// 反映した X ボタンの二回押しの回数（[`kernel::PanelStatus::double_press`]）。
    toggles: u32,
    /// 前回見たときにロックダウン中だったか（入った瞬間にダッシュボードを隠す）。
    locked: bool,
    /// 付いている手首（利き手の反対。設定が変われば自分で付け替える）。
    hand: Hand,
    /// 掴んで置き直せるようにしてある（ダッシュボードが出ている間）。
    movable: bool,
    /// 前回見たときに掴まれていたか（離した瞬間に置き場所を覚える）。
    grabbed: bool,
}

impl Wrist {
    pub fn new(
        panel: PanelId,
        debug: dashboard::Summon,
        settings: dashboard::Summon,
        cx: &mut Context<Self>,
    ) -> Self {
        // 時計を進める: 1 秒ごとに描き直す。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |_, cx| ui::notify(panel, cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
        cx.observe_global::<PanelControl>(Self::sync_dashboard)
            .detach();
        cx.observe_global::<PanelControl>(Self::follow_dominant_hand)
            .detach();
        cx.observe_global::<PanelControl>(Self::remember_placement)
            .detach();
        let hand = off_hand(cx);
        // 文字盤を選ぶ画面（設定など）が見本を描けるように。画面どうしは直接依存しないので、UI 層のグローバルに置く。
        cx.set_global(ui::prefs::FacePreview(face::preview));
        // ダッシュボードは起動時に隠したまま作っておく（画面の配信を用意しておき、X の二回押しでは出すだけ）。
        open_dashboard(settings, debug, true, cx);
        // OS の通知のトースト（隠したまま。届くと視界の端に出て、この時計・ダッシュボードの上へ動く）。
        let wrist = cx.entity();
        let toast = toast::spec(panel, move |id, cx| {
            wrist.update(cx, |w, _| w.toast = Some(id));
        });
        cx.default_global::<PanelControl>().spawns.push(toast);
        Self {
            debug,
            settings,
            dashboard: None,
            toast: None,
            panel,
            toggles: 0,
            locked: false,
            hand,
            movable: false,
            grabbed: false,
        }
    }

    /// 掴んで離したら、置き直した姿勢（左手首での値に直して）と大きさを設定に書き戻す（次の起動も同じ所・同じ大きさに付く）。
    /// フレームループが状態を書き戻すたびに呼ばれるので、離した瞬間のほかは何もしない（書き込むと自分がまた呼ばれる）。
    fn remember_placement(&mut self, cx: &mut Context<Self>) {
        let Some(state) = cx.global::<PanelControl>().panels.get(&self.panel) else {
            return;
        };
        let (grabbed, offset, size) = (state.status.grabbed, state.offset, state.size);
        let released = self.grabbed && !grabbed;
        self.grabbed = grabbed;
        if !released {
            return;
        }
        if let Some(offset) = offset {
            ui::set(Setting::WristOffset(mirror(offset, self.hand)), cx);
        }
        ui::set(Setting::WristScale(size.x / SIZE_M.x), cx);
    }

    /// 利き手の設定が変わったら、反対の手首へ付け替える（変わっていなければ何もしない。書き込むと自分がまた呼ばれる）。
    fn follow_dominant_hand(&mut self, cx: &mut Context<Self>) {
        let hand = off_hand(cx);
        if hand == self.hand {
            return;
        }
        self.hand = hand;
        let attach =
            PanelCommand::Attach(Device::Controller(hand), offset(hand, &ui::settings(cx)));
        ui::command(self.panel, attach, cx);
    }

    /// X ボタンを二回押すたびにダッシュボードを出す・隠す（閉じられていれば開き直す。音はダッシュボード自身が鳴らす）。
    /// フレームループが状態を書き戻すたびに呼ばれるので、変わっていなければ何もしない（書き込むと自分がまた呼ばれる）。
    fn sync_dashboard(&mut self, cx: &mut Context<Self>) {
        let ctl = cx.global::<PanelControl>();
        let toggles = ctl
            .panels
            .get(&self.panel)
            .map_or(0, |p| p.status.double_press);
        let locked = ctl.settings.lockdown;
        let open = self
            .dashboard
            .map(|id| (id, ctl.panels.get(&id).map(|p| p.status.clone())))
            .filter(|(_, s)| !s.as_ref().is_some_and(|s| s.closed));
        // ロックダウンに入った瞬間、出ているダッシュボードを隠す（解けても出し直さない）。
        let entered = locked && !self.locked;
        // ロックダウンの出入りに合わせて、手首自身も隠す・出す。
        let changed = locked != self.locked;
        self.locked = locked;
        if changed {
            cx.default_global::<PanelControl>()
                .panels
                .entry(self.panel)
                .or_default()
                .commands
                .push(kernel::PanelCommand::Hide(locked));
        }
        if entered
            && let Some((id, status)) = &open
            && !status.as_ref().is_some_and(|s| s.hidden)
        {
            dashboard::hide(*id, cx);
        }
        // ダッシュボードが出ている間だけ掴める。掴んだまま消えたら、ロックがその掴みを取り消して元の位置へ戻す。
        let movable = open
            .as_ref()
            .is_some_and(|(_, s)| s.as_ref().is_some_and(|s| !s.hidden));
        if movable != self.movable {
            self.movable = movable;
            ui::command(self.panel, PanelCommand::Lock(!movable), cx);
        }
        if toggles == self.toggles {
            return;
        }
        self.toggles = toggles;
        // ロックダウン中と、開閉を許さない案内の最中はダッシュボードを開閉しない（押したことだけ受け取って捨てる）。
        if locked || !ui::tour::may_toggle(cx) {
            return;
        }
        match open {
            Some((id, status)) if status.as_ref().is_some_and(|s| s.hidden) => {
                dashboard::show(id, cx)
            }
            Some((id, _)) => dashboard::hide(id, cx),
            None => open_dashboard(self.settings, self.debug, false, cx),
        }
    }
}

/// 今の設定で wrist を付ける手（利き手の反対）。
fn off_hand(cx: &gpui::App) -> Hand {
    ui::settings(cx).dominant_hand.off_hand()
}

/// ダッシュボードを開く（`hidden` なら隠したまま）。開いたら [`Wrist::dashboard`] に覚える。
fn open_dashboard(
    settings: dashboard::Summon,
    debug: dashboard::Summon,
    hidden: bool,
    cx: &mut Context<Wrist>,
) {
    let wrist = cx.entity();
    let spec = dashboard::spec(
        move |id, cx| {
            wrist.update(cx, |w, _| w.dashboard = Some(id));
        },
        settings,
        debug,
        hidden,
    );
    cx.default_global::<PanelControl>().spawns.push(spec);
}

impl Render for Wrist {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = chrono::Local::now();
        let theme = Theme::current(cx);
        let design = ui::settings(cx).wrist_design;
        // 腕時計の文字盤: 丸いカード 1 枚がパネル全体（地は敷かない）。中身は選んだデザイン。
        components::card_panel(&theme)
            .rounded(px(radius::FULL))
            // 腕時計の文字盤は正円。全域のスーパーエリプスからは外す。
            .circular_corners()
            .child(face::dial(
                design,
                &theme,
                now,
                i18n::date(cx, now).into(),
                face::BASE,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use kernel::DominantHand;

    fn wrist(cx: &mut Context<Wrist>) -> Wrist {
        Wrist::new(
            0,
            |f, _| PanelSpec::spawn(f, |_, _, _| gpui::Empty),
            |f, _| PanelSpec::spawn(f, |_, _, _| gpui::Empty),
            cx,
        )
    }

    /// ダッシュボードは起動時に隠したまま開き、X ボタンを二回押すたびに出す・隠すを繰り返す（閉じない）。
    #[gpui::test]
    fn double_x_shows_and_hides_the_dashboard_opened_at_startup(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| wrist(cx));
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
        // 起動時に隠したまま開いておく（ダッシュボードと通知のトースト）。
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            2
        );
        wrist.update(cx, |w, _| w.dashboard = Some(3));
        let set_hidden = |cx: &mut gpui::VisualTestContext, hidden| {
            cx.update(|_, cx| {
                cx.default_global::<PanelControl>()
                    .panels
                    .entry(3)
                    .or_default()
                    .status
                    .hidden = hidden
            })
        };
        set_hidden(cx, true);
        press_twice(cx, 1);
        assert!(take(cx, 3).contains(&PanelCommand::Hide(false)));
        assert!(take(cx, 0).is_empty(), "音は手首からは鳴らさない");
        set_hidden(cx, false);
        press_twice(cx, 2);
        assert!(take(cx, 3).contains(&PanelCommand::Hide(true)));
        set_hidden(cx, true);
        press_twice(cx, 3);
        assert!(take(cx, 3).contains(&PanelCommand::Hide(false)));
        assert_eq!(wrist.read_with(cx, |w, _| w.dashboard), Some(3));
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            2,
            "開き直さない"
        );
    }

    /// ロックダウン中は X ボタンを二回押してもダッシュボードを出さない（押したことは受け取る）。
    #[gpui::test]
    fn double_x_does_nothing_in_lockdown(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| wrist(cx));
        cx.run_until_parked();
        wrist.update(cx, |w, _| w.dashboard = Some(3));
        cx.update(|_, cx| {
            let ctl = cx.default_global::<PanelControl>();
            ctl.settings.lockdown = true;
            ctl.panels.entry(3).or_default().status.hidden = true;
            ctl.panels.entry(0).or_default().status.double_press = 1;
        });
        cx.run_until_parked();
        let commands = cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(3)
                .or_default()
                .commands
                .clone()
        });
        assert!(commands.is_empty(), "{commands:?}");
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            2,
            "開かない"
        );
    }

    /// ロックダウンに入ったとき、出ていたダッシュボードは隠れる（1 回だけ。解けても出し直さない）。
    #[gpui::test]
    fn lockdown_hides_a_visible_dashboard_once(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| wrist(cx));
        cx.run_until_parked();
        wrist.update(cx, |w, _| w.dashboard = Some(3));
        let commands = |cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| {
                std::mem::take(
                    &mut cx
                        .default_global::<PanelControl>()
                        .panels
                        .entry(3)
                        .or_default()
                        .commands,
                )
            })
        };
        let set_lockdown = |cx: &mut gpui::VisualTestContext, on| {
            cx.update(|_, cx| {
                let ctl = cx.default_global::<PanelControl>();
                ctl.settings.lockdown = on;
                ctl.panels.entry(3).or_default().status.hidden = false;
            });
            cx.run_until_parked();
        };
        // 出ているダッシュボード（hidden = false）の前でロックダウンに入る。
        set_lockdown(cx, true);
        assert!(commands(cx).contains(&PanelCommand::Hide(true)));
        // 続けて状態が書かれても、もう一度は隠さない。
        set_lockdown(cx, true);
        assert!(commands(cx).is_empty());
        // 解けても出し直さない。
        set_lockdown(cx, false);
        assert!(commands(cx).is_empty());
    }

    /// ロックダウン中は手首自身も隠れ、解けると出る。
    #[gpui::test]
    fn lockdown_hides_the_wrist_itself(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| wrist(cx));
        cx.run_until_parked();
        let commands = |cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| {
                std::mem::take(
                    &mut cx
                        .default_global::<PanelControl>()
                        .panels
                        .entry(0)
                        .or_default()
                        .commands,
                )
            })
        };
        let set_lockdown = |cx: &mut gpui::VisualTestContext, on| {
            cx.update(|_, cx| cx.default_global::<PanelControl>().settings.lockdown = on);
            cx.run_until_parked();
        };
        commands(cx);
        set_lockdown(cx, true);
        assert!(commands(cx).contains(&PanelCommand::Hide(true)));
        set_lockdown(cx, false);
        assert!(commands(cx).contains(&PanelCommand::Hide(false)));
    }

    /// ダッシュボードが出ている間だけ掴める（出ると `Lock(false)`、隠れると `Lock(true)`。変わらなければ積まない）。
    #[gpui::test]
    fn grabbable_only_while_the_dashboard_shows(cx: &mut TestAppContext) {
        let (wrist, cx) = cx.add_window_view(|_, cx| wrist(cx));
        cx.run_until_parked();
        wrist.update(cx, |w, _| w.dashboard = Some(3));
        let mut set_hidden = |hidden| {
            cx.update(|_, cx| {
                let ctl = cx.default_global::<PanelControl>();
                ctl.panels.entry(3).or_default().status.hidden = hidden;
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                let ctl = cx.default_global::<PanelControl>();
                std::mem::take(&mut ctl.panels.entry(0).or_default().commands)
            })
        };
        assert_eq!(set_hidden(false), [PanelCommand::Lock(false)]);
        assert_eq!(set_hidden(false), []);
        assert_eq!(set_hidden(true), [PanelCommand::Lock(true)]);
    }

    /// 利き手を替えると、反対の手首へ自分で付け替える（設定が同じままなら何もしない）。
    #[gpui::test]
    fn follows_the_dominant_hand_to_the_other_wrist(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| wrist(cx));
        cx.run_until_parked();
        let mut set = |hand| {
            cx.update(|_, cx| {
                cx.default_global::<PanelControl>().settings.dominant_hand = hand;
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                let ctl = cx.default_global::<PanelControl>();
                std::mem::take(&mut ctl.panels.entry(0).or_default().commands)
            })
        };
        let attach = |hand| PanelCommand::Attach(Device::Controller(hand), watch_offset(hand));
        assert_eq!(set(DominantHand::Left), [attach(Hand::Right)]);
        assert_eq!(set(DominantHand::Left), []);
        assert_eq!(set(DominantHand::Right), [attach(Hand::Left)]);
    }

    /// 掴んで離した瞬間だけ、置き直した姿勢と大きさの倍率を設定に書き戻す。右手首では左手首での値（鏡写し）に直して覚える。
    #[gpui::test]
    fn remembers_the_placement_when_released(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| wrist(cx));
        cx.run_until_parked();
        let placed = Pose::new(Vec3::new(0.05, 0.1, -0.02), Quat::from_rotation_z(0.4));
        let grab = |grabbed, cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| {
                let state = cx.default_global::<PanelControl>().panels.entry(0);
                let state = state.or_default();
                state.status.grabbed = grabbed;
                state.offset = Some(placed);
                state.size = SIZE_M * 1.5;
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                std::mem::take(&mut cx.default_global::<PanelControl>().setting_changes)
            })
        };
        let saved = |changes: Vec<Setting>| match changes[..] {
            [Setting::WristOffset(o), Setting::WristScale(s)] if (s - 1.5).abs() < 1e-5 => o,
            _ => panic!("{changes:?}"),
        };
        assert_eq!(grab(true, cx), [], "掴んでいる間は覚えない");
        assert_eq!(saved(grab(false, cx)), placed);
        assert_eq!(grab(false, cx), [], "離した瞬間だけ");
        // 利き手を左にすると右手首に付く。
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>().settings.dominant_hand = DominantHand::Left;
        });
        cx.run_until_parked();
        grab(true, cx);
        assert_eq!(saved(grab(false, cx)), mirror(placed, Hand::Right));
    }

    /// 覚えた姿勢が無ければ既定の腕時計の所。あれば左手首ではそのまま、右手首では鏡写しで付ける。
    #[test]
    fn the_saved_offset_is_mirrored_onto_the_right_wrist() {
        let eq = |a: Pose, b: Pose| {
            assert!(a.position.abs_diff_eq(b.position, 1e-6), "{a:?} != {b:?}");
            assert!(
                a.orientation.abs_diff_eq(b.orientation, 1e-6),
                "{a:?} != {b:?}"
            );
        };
        let mut saved = Settings::default();
        for hand in [Hand::Left, Hand::Right] {
            assert_eq!(offset(hand, &saved), watch_offset(hand));
        }
        // 既定の左右も鏡写しどうし（覚えた値を左手首での値にそろえても、既定の所は変わらない）。
        eq(
            mirror(watch_offset(Hand::Left), Hand::Right),
            watch_offset(Hand::Right),
        );
        let placed = Pose::new(Vec3::new(-0.05, 0.1, 0.02), Quat::from_rotation_z(0.4));
        saved.wrist_offset = Some(placed);
        assert_eq!(offset(Hand::Left, &saved), placed);
        let right = offset(Hand::Right, &saved);
        assert_eq!(right.position, Vec3::new(0.05, 0.1, 0.02));
        eq(mirror(right, Hand::Right), placed);
    }

    /// 腕時計を見る姿勢（腕を胸の前で横にし、手の甲を上の顔へ向ける）で、時計は手首の甲の側・腕寄りにあり、表が上を向き、
    /// 12 時が向こう・3 時が右で正立して読める。骨格だけの手の取り付け先（`grip`）は握ったコントローラの tip に当たるので、
    /// コントローラの手首も同じく確かめている。
    #[test]
    fn the_watch_reads_upright_on_the_back_of_the_wrist() {
        use kernel::vr::{FingerTracking, HandSkeleton, JOINT_COUNT, WRIST};
        for hand in [Hand::Left, Hand::Right] {
            // 指先は左手なら右（+X）、右手なら左（-X）。手のひらは下、親指は手前（+Z）。
            let dir = if hand == Hand::Left { 1.0 } else { -1.0 };
            let mut joints = [Pose::IDENTITY; JOINT_COUNT];
            // 中指・人差し指・小指の付け根。
            joints[12].position = Vec3::new(dir * 0.08, 0.0, 0.0);
            joints[7].position = Vec3::new(dir * 0.08, 0.0, 0.03);
            joints[22].position = Vec3::new(dir * 0.07, 0.0, -0.03);
            let skeleton = HandSkeleton {
                hand,
                joints,
                tracking: FingerTracking::Full,
            };
            let watch = skeleton.grip() * watch_offset(hand);
            let eq = |a: Vec3, b: Vec3| assert!(a.abs_diff_eq(b, 1e-5), "{hand:?}: {a} != {b}");
            let wrist = joints[WRIST].position;
            eq(watch.position, wrist + Vec3::new(-dir * 0.03, LIFT, 0.0));
            eq(watch.orientation * Vec3::Z, Vec3::Y);
            eq(watch.orientation * Vec3::Y, Vec3::NEG_Z);
            eq(watch.orientation * Vec3::X, Vec3::X);
        }
    }

    /// 設定のデザインで文字盤が替わる。どのデザインも日付を出し、針の無い種類は時刻を数字で出す。
    #[gpui::test]
    fn the_face_follows_the_wrist_design(cx: &mut TestAppContext) {
        use kernel::WristFace;
        let (_, cx) = cx.add_window_view(|_, cx| wrist(cx));
        for &face in WristFace::ALL {
            for &design in face.designs() {
                cx.update(|window, cx| {
                    cx.default_global::<PanelControl>().settings.wrist_design = design;
                    window.refresh();
                });
                cx.run_until_parked();
                assert!(cx.debug_bounds("date").is_some(), "{design:?}");
                let digits = cx.debug_bounds("time").is_some();
                assert_eq!(digits, face != WristFace::Analog, "{design:?}");
            }
        }
    }

    /// 丸い文字盤の中央に、日付・時刻を縦に並べる。
    #[gpui::test]
    fn layout_is_date_over_time(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| wrist(cx));
        cx.run_until_parked();
        let [d, t] = ["date", "time"].map(|s| cx.debug_bounds(s).unwrap());
        assert!(d.bottom() <= t.top());
        let mid = |r: gpui::Bounds<gpui::Pixels>| r.center().x;
        assert!((mid(d) - mid(t)).abs() < px(1.0));
    }
}
