//! ツールチップ。カーソルの子のパネル（[`Pointer::Tooltip`]）に、指している要素の説明を描く。
//! パネルの中心がカーソルで、箱はオーバーレイ層が決めた向き（[`Anchor`]）に出す。
//!
//! - 出るときは向きの側へ、カーソルから離れるようにスライドしながら現れる（[`Motion::Enter`]、ease-out）。消えるときは逆。
//! - 要素から離れても [`GRACE`] の間は残り、その間に別の要素を指せば文字を変える（箱の幅は [`Motion::Move`] で伸び縮みする）。
//! - 向きが変わったら、今の側で消えてから新しい側で出る。

use std::time::{Duration, Instant};

use design::animation::{Animation, Motion};
use design::components::{TOOLTIP_HEIGHT, tooltip, tooltip_width};
use gpui::{Context, SharedString, Window, div, prelude::*, px};
use kernel::{Anchor, Hand, PanelId, Pointer};

/// 要素から離れてから、消え始めるまでの猶予。
pub const GRACE: Duration = Duration::from_millis(125);
/// スライドする距離（px）。
const SLIDE: f32 = design::spacing::SM;

/// 今の表示。
#[derive(Debug)]
pub struct State {
    /// 描いている文字と向き。消えきるまで残る。
    pub shown: Option<(SharedString, Anchor)>,
    /// 見えている度合い（0〜1）と、向かっている先。
    pub vis: f32,
    vis_to: f32,
    vis_anim: Option<Animation>,
    /// 箱の幅（px）と、文字を変えたときの伸び縮み。
    pub width: f32,
    width_anim: Option<Animation>,
    /// 要素から離れた時刻（猶予の始まり）。
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
            left: None,
            last: None,
        }
    }
}

impl State {
    /// 1 回分進める。`request` は今指している要素の文字、`anchor` は出したい向き、`animated` が偽なら動かさずに切り替える。
    /// `measure` は文字の箱の幅。まだ動いている・猶予を数えているなら `true`（次のフレームも描く）。
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

        // 出したい文字: 指していればそれ、離れてから猶予の間は今の文字のまま。
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
                    if animated {
                        self.width_anim = Some(Animation::new(self.width, to, Motion::Move));
                    } else {
                        self.width = to;
                    }
                }
                self.go(1.0, Motion::Enter, animated);
            }
            // 向きが変わった: 今の側で消えきってから、新しい側で出る。
            (Some(_), Some(_)) if self.vis > 0.0 => self.go(0.0, Motion::Exit, animated),
            (Some(t), _) => {
                self.width = measure(&t);
                self.width_anim = None;
                self.shown = Some((t, anchor));
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

/// `hand` のツールチップを描くパネル。
pub struct Tooltip {
    pub hand: Hand,
    pub panel: PanelId,
    pub state: State,
}

/// 箱の端をカーソルの中心から離す距離（px）。カーソルの輪の外側から `spacing::SM` 空ける。
fn gap() -> f32 {
    let ring = super::CURSOR_M * (super::RING_RADIUS_PX + super::RING_WIDTH_PX / 2.0)
        / super::CURSOR_PX.0 as f32;
    ring * super::TOOLTIP_PX.0 as f32 / super::TOOLTIP_M + design::spacing::SM
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
        let busy = self
            .state
            .step(Instant::now(), request.as_ref(), anchor, animated, |t| {
                tooltip_width(t, window).min(max)
            });
        // 動いている・猶予を数えている間は毎フレーム描き直してもらう。
        cx.default_global::<ui::PanelControl>()
            .panels
            .entry(self.panel)
            .or_default()
            .live = busy;
        let s = &self.state;
        let (w, h) = (super::TOOLTIP_PX.0 as f32, super::TOOLTIP_PX.1 as f32);
        let hide = (1.0 - s.vis) * SLIDE;
        let top = match s.shown.as_ref().map(|(_, a)| *a) {
            // 上に出る箱は下から（カーソルの側から）上がってくる。下はその逆。
            Some(Anchor::Above) => h / 2.0 - gap() - TOOLTIP_HEIGHT + hide,
            _ => h / 2.0 + gap() - hide,
        };
        div()
            .size_full()
            .debug_selector(|| "tooltip".into())
            .when_some(s.shown.clone(), |d, (text, _)| {
                d.child(
                    tooltip(text, s.width)
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
        // ease-out: 前半で半分以上出ている。
        assert!(mid > 0.5 && mid < 1.0, "{mid}");
        assert!(
            !run(&mut s, t0, 60, 400, Some("Save"), Anchor::Above),
            "止まれば描き直さない"
        );
        assert_eq!((s.vis, s.width), (1.0, 40.0));
    }

    #[test]
    fn stays_for_the_grace_then_fades_out() {
        let (t0, mut s) = (Instant::now(), State::default());
        run(&mut s, t0, 0, 400, Some("Save"), Anchor::Above);
        // 離れて猶予の間は残る（猶予を数えるため描き直し続ける）。
        assert!(run(&mut s, t0, 410, 450, None, Anchor::Above));
        assert_eq!(s.vis, 1.0);
        // 猶予が過ぎたら消える。
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
        assert_eq!(s.vis, 1.0, "消えずに文字だけ変わる");
        assert!(
            s.width > 40.0 && s.width < 130.0,
            "幅は動いている途中: {}",
            s.width
        );
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
            "消えきるまでは元の側"
        );
        assert!(s.vis < 1.0);
        run(&mut s, t0, 460, 1000, Some("Save"), Anchor::Below);
        assert_eq!(s.shown.as_ref().unwrap().1, Anchor::Below);
        assert_eq!(s.vis, 1.0);
    }

    #[test]
    fn without_motion_it_switches_at_once() {
        let (t0, mut s) = (Instant::now(), State::default());
        let req = SharedString::from("Save");
        s.step(t0, Some(&req), Anchor::Below, false, |_| 40.0);
        assert_eq!((s.vis, s.width), (1.0, 40.0));
        s.step(t0 + MS(10), None, Anchor::Below, false, |_| 40.0);
        assert!(s.shown.is_some(), "猶予の間は残る");
        s.step(t0 + MS(10) + GRACE, None, Anchor::Below, false, |_| 40.0);
        assert!(s.shown.is_none());
    }
}
