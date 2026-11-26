//! ツールチップ。カーソル子パネル（[`Pointer::Tooltip`]）にホバー中の要素の説明を描画する。
//! パネル中心がカーソル位置で、ボックスはオーバーレイ層が決定した方向（[`Anchor`]）に配置される。
//!
//! - 出現時はアンカー方向へカーソルから離れるようにスライドしながらフェードインする（[`Motion::Enter`]、ease-out）。退出時は逆。
//! - 要素から離れても [`GRACE`] 期間は残存し、その間に別の要素をホバーすればテキストを切り替える（ボックス幅は [`Motion::Move`] で遷移する）。
//! - アンカー方向が変更された場合、現在側で退出してから新しい側で出現する。

use std::time::{Duration, Instant};

use design::animation::{Animation, Motion};
use design::components::{TOOLTIP_HEIGHT, tooltip, tooltip_width};
use gpui::{Context, SharedString, Window, div, prelude::*, px};
use kernel::{Anchor, Hand, PanelId, Pointer};

/// 要素から離れてから退出アニメーション開始までの猶予期間。
pub const GRACE: Duration = Duration::from_millis(125);
/// スライド距離（px）。
const SLIDE: f32 = design::spacing::SM;

/// 現在の表示状態。
#[derive(Debug)]
pub struct State {
    /// 表示中のテキストとアンカー方向。退出アニメーション完了まで保持される。
    pub shown: Option<(SharedString, Anchor)>,
    /// 可視度（0〜1）と遷移先の値。
    pub vis: f32,
    vis_to: f32,
    vis_anim: Option<Animation>,
    /// ボックスの幅（px）とテキスト変更時のリサイズアニメーション。
    pub width: f32,
    width_anim: Option<Animation>,
    /// 現在のテキストの表示開始時刻（マーキースクロールの基準時刻）。
    pub since: Option<Instant>,
    /// 要素から離れた時刻（猶予期間の開始）。
    left: Option<Instant>,
    last: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            shown: None,
            vis: 0.0,
            vis_to: 0.0,
            vis_anim: None,
            width: 0.0,
            width_anim: None,
            since: None,
            left: None,
            last: None,
        }
    }
}

impl State {
    /// 1 フレーム分進める。`request` はホバー中の要素のテキスト、`anchor` はアンカー方向、`animated` が偽なら即座に切り替える。
    /// `measure` はテキストのボックス幅を返す。遷移中または猶予カウント中なら `true` を返す（次フレームも再描画が必要）。
    pub fn step(
        &mut self,
        now: Instant,
        request: Option<&SharedString>,
        anchor: Anchor,
        animated: bool,
        mut measure: impl FnMut(&SharedString) -> f32,
    ) -> bool {
        let dt = self
            .last
            .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        self.last = Some(now);
        for (value, anim) in [
            (&mut self.vis, &mut self.vis_anim),
            (&mut self.width, &mut self.width_anim),
        ] {
            if let Some(a) = anim {
                *value = a.advance(dt);
                if a.is_done() {
                    *anim = None;
                }
            }
        }

        // 表示対象のテキスト: ホバー中ならそのテキスト、離れてから猶予期間中は現在のテキストを維持。
        let wanted = match request {
            Some(t) => {
                self.left = None;
                Some(t.clone())
            }
            None => self.shown.as_ref().and_then(|(t, _)| {
                let left = *self.left.get_or_insert(now);
                (now.saturating_duration_since(left) < GRACE).then(|| t.clone())
            }),
        };

        match (wanted, &mut self.shown) {
            (None, _) => {
                self.go(0.0, Motion::Exit, animated);
                if self.vis == 0.0 {
                    self.shown = None;
                    self.left = None;
                }
            }
            (Some(t), Some((text, side))) if *side == anchor => {
                if *text != t {
                    let to = measure(&t);
                    *text = t;
                    self.since = Some(now);
                    if animated {
                        self.width_anim = Some(Animation::new(self.width, to, Motion::Move));
                    } else {
                        self.width = to;
                    }
                }
                self.go(1.0, Motion::Enter, animated);
            }
            // アンカー方向変更: 現在側で完全に退出してから新しい側で出現する。
            (Some(_), Some(_)) if self.vis > 0.0 => self.go(0.0, Motion::Exit, animated),
            (Some(t), _) => {
                self.width = measure(&t);
                self.width_anim = None;
                self.shown = Some((t, anchor));
                self.since = Some(now);
                self.vis_to = 0.0;
                self.go(1.0, Motion::Enter, animated);
            }
        }
        self.vis_anim.is_some()
            || self.width_anim.is_some()
            || self.shown.is_some() && request.is_none()
    }

    fn go(&mut self, to: f32, motion: Motion, animated: bool) {
        if self.vis_to == to {
            return;
        }
        self.vis_to = to;
        if animated {
            self.vis_anim = Some(Animation::new(self.vis, to, motion));
        } else {
            (self.vis, self.vis_anim) = (to, None);
        }
    }
}

/// `hand` のツールチップ描画パネル。
pub struct Tooltip {
    pub hand: Hand,
    pub panel: PanelId,
    pub state: State,
}

/// ボックス端とカーソル中心の距離（px）。スケール倍率 `cursor` を乗じたカーソルリング外縁から `spacing::SM` の余白を取る。
fn gap(cursor: f32) -> f32 {
    let ring = super::CURSOR_M * (super::RING_RADIUS_PX + super::RING_WIDTH_PX / 2.0)
        / super::CURSOR_PX.0 as f32;
    cursor * ring * super::TOOLTIP_PX.0 as f32 / super::TOOLTIP_M + design::spacing::SM
}

/// ボックス上端の Y 座標（px）。アンカー `anchor` の方向にカーソルから `gap` 離し、非表示度 `hide` 分だけカーソル側に戻す。
fn top(anchor: Option<Anchor>, gap: f32, hide: f32) -> f32 {
    let h = super::TOOLTIP_PX.1 as f32;
    match anchor {
        // 上方配置のボックスは下側（カーソル側）からスライドイン。下方配置はその逆。
        Some(Anchor::Above) => h / 2.0 - gap - TOOLTIP_HEIGHT + hide,
        _ => h / 2.0 + gap - hide,
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ctl = cx.try_global::<ui::PanelControl>();
        let request = ctl.and_then(|c| c.tooltip.get(&self.hand)).cloned();
        let anchor = ctl
            .and_then(|c| c.anchor.get(&self.hand))
            .copied()
            .unwrap_or_default();
        let animated = cx.has_global::<design::Motion>();
        let max = super::TOOLTIP_PX.0 as f32;
        let now = Instant::now();
        let busy = self
            .state
            .step(now, request.as_ref(), anchor, animated, |t| {
                tooltip_width(t, window).min(max)
            });
        let s = &self.state;
        // 収まらないテキストはマーキースクロールする。
        let scroll = s.shown.as_ref().zip(s.since).and_then(|((t, _), since)| {
            let t0 = now.saturating_duration_since(since).as_secs_f32();
            design::components::tooltip_scroll(tooltip_width(t, window), s.width, t0)
        });
        // 遷移中・猶予カウント中・スクロール中は毎フレームの再描画を要求する。
        cx.default_global::<ui::PanelControl>()
            .panels
            .entry(self.panel)
            .or_default()
            .live = busy || scroll.is_some();
        let w = super::TOOLTIP_PX.0 as f32;
        let hide = (1.0 - s.vis) * SLIDE;
        let gap = gap(ui::settings(cx).cursor_scale);
        let top = top(s.shown.as_ref().map(|(_, a)| *a), gap, hide);
        div()
            .size_full()
            .debug_selector(|| "tooltip".into())
            .when_some(s.shown.clone(), |d, (text, _)| {
                d.child(
                    tooltip(text, s.width, scroll)
                        .absolute()
                        .left(px((w - s.width) / 2.0))
                        .top(px(top))
                        .opacity(s.vis),
                )
            })
    }
}

/// [`Pointer::Tooltip`] のパネル。
pub fn spec(hand: Hand) -> ui::PanelSpec {
    ui::PanelSpec::root(kernel::Pose::IDENTITY, move |panel, _, _| Tooltip {
        hand,
        panel,
        state: State::default(),
    })
    .sized(
        kernel::Vec2::new(
            super::TOOLTIP_M,
            super::TOOLTIP_M * super::TOOLTIP_PX.1 as f32 / super::TOOLTIP_PX.0 as f32,
        ),
        super::TOOLTIP_PX,
    )
    .pointer(hand, Pointer::Tooltip)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn run(
        s: &mut State,
        t0: Instant,
        from_ms: u64,
        to_ms: u64,
        req: Option<&str>,
        anchor: Anchor,
    ) -> bool {
        let req = req.map(SharedString::from);
        let mut busy = false;
        for ms in (from_ms..=to_ms).step_by(10) {
            busy = s.step(t0 + MS(ms), req.as_ref(), anchor, true, |t| {
                t.len() as f32 * 10.0
            });
        }
        busy
    }

    #[test]
    fn slides_in_then_settles() {
        let (t0, mut s) = (Instant::now(), State::default());
        let req = SharedString::from("Save");
        s.step(t0, Some(&req), Anchor::Above, true, |_| 40.0);
        assert_eq!(s.vis, 0.0);
        let mid = {
            s.step(t0 + MS(50), Some(&req), Anchor::Above, true, |_| 40.0);
            s.vis
        };
        // ease-out: 前半で半分以上の可視度に到達する。
        assert!(mid > 0.5 && mid < 1.0, "{mid}");
        assert!(
            !run(&mut s, t0, 60, 400, Some("Save"), Anchor::Above),
            "停止後は再描画不要"
        );
        assert_eq!((s.vis, s.width), (1.0, 40.0));
    }

    #[test]
    fn stays_for_the_grace_then_fades_out() {
        let (t0, mut s) = (Instant::now(), State::default());
        run(&mut s, t0, 0, 400, Some("Save"), Anchor::Above);
        // 離れて猶予期間中は残存する（猶予カウントのため再描画を継続する）。
        assert!(run(&mut s, t0, 410, 450, None, Anchor::Above));
        assert_eq!(s.vis, 1.0);
        // 猶予期間経過後に退出する。
        run(&mut s, t0, 470, 1000, None, Anchor::Above);
        assert!(s.shown.is_none() && s.vis == 0.0);
    }

    #[test]
    fn a_new_element_within_the_grace_swaps_the_text_and_resizes() {
        let (t0, mut s) = (Instant::now(), State::default());
        run(&mut s, t0, 0, 400, Some("Save"), Anchor::Above);
        run(&mut s, t0, 410, 440, None, Anchor::Above);
        run(&mut s, t0, 450, 460, Some("Open settings"), Anchor::Above);
        assert_eq!(s.shown.as_ref().unwrap().0, "Open settings");
        assert_eq!(s.vis, 1.0, "退出せずテキストのみ切り替わる");
        assert!(s.width > 40.0 && s.width < 130.0, "幅は遷移中: {}", s.width);
        run(&mut s, t0, 470, 900, Some("Open settings"), Anchor::Above);
        assert_eq!(s.width, 130.0);
    }

    #[test]
    fn flipping_the_anchor_exits_then_enters_on_the_new_side() {
        let (t0, mut s) = (Instant::now(), State::default());
        run(&mut s, t0, 0, 400, Some("Save"), Anchor::Above);
        run(&mut s, t0, 410, 450, Some("Save"), Anchor::Below);
        assert_eq!(
            s.shown.as_ref().unwrap().1,
            Anchor::Above,
            "退出完了まで元のアンカー方向を維持"
        );
        assert!(s.vis < 1.0);
        run(&mut s, t0, 460, 1000, Some("Save"), Anchor::Below);
        assert_eq!(s.shown.as_ref().unwrap().1, Anchor::Below);
        assert_eq!(s.vis, 1.0);
    }

    /// カーソルを最大サイズにしても、ボックスはリングに重ならず、パネルからもはみ出さない（上下いずれの方向でも）。
    #[test]
    fn box_clears_the_cursor_ring_and_stays_in_the_panel_at_any_cursor_scale() {
        let h = super::super::TOOLTIP_PX.1 as f32;
        for cursor in [kernel::CURSOR_SCALE_MIN, 1.0, kernel::CURSOR_SCALE_MAX] {
            let gap = gap(cursor);
            assert!(gap > design::spacing::SM, "{cursor}");
            assert!(top(Some(Anchor::Above), gap, 0.0) >= 0.0, "{cursor}");
            assert!(
                top(Some(Anchor::Below), gap, 0.0) + TOOLTIP_HEIGHT <= h,
                "{cursor}"
            );
        }
        assert!(gap(2.0) > gap(1.0), "カーソルが大きいほど距離が増加する");
    }

    #[test]
    fn without_motion_it_switches_at_once() {
        let (t0, mut s) = (Instant::now(), State::default());
        let req = SharedString::from("Save");
        s.step(t0, Some(&req), Anchor::Below, false, |_| 40.0);
        assert_eq!((s.vis, s.width), (1.0, 40.0));
        s.step(t0 + MS(10), None, Anchor::Below, false, |_| 40.0);
        assert!(s.shown.is_some(), "猶予期間中は残存する");
        s.step(t0 + MS(10) + GRACE, None, Anchor::Below, false, |_| 40.0);
        assert!(s.shown.is_none());
    }
}
