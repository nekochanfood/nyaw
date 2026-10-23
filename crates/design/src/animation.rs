//! アニメーション。animato をここで包み、呼び出し側は [`Motion`] と [`Animation`] だけを見る。
//! 長さは `duration_ms`（tokens.json）、イージングは用途ごとに 1 つ決めてある。

use std::time::Instant;

use animato::{Easing, Tween, Update};

use crate::duration_ms;

/// ボタンの滑り込み・滑り出しの長さ（ms）。
const SLIDE_MS: u64 = 350;

/// 動きの用途。長さとイージングはここで決まる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// 現れる。ゆっくり着地する（ease-out）。
    Enter,
    /// 消える。速く離れる（ease-in）。
    Exit,
    /// 元に戻る。消えるときと同じ長さで、終わり際にゆっくり着地する（ease-out）。
    Return,
    /// 位置や明るさの移り変わり（ease-in-out）。
    Move,
    /// 移動と大きさの変化が続く、形の移り変わり（ease-in-out。段が多いので一番遅い長さ）。
    Morph,
    /// ボタンが滑り込む。少し行き過ぎてから収まる（ease-out-back）。
    SlideIn,
    /// ボタンが滑り出る。滑り込みの逆の動きで、少し引いてから下へ引っ込む（ease-in-back。滑り込みと同じ長さ）。
    SlideOut,
    /// ボタンの幅が開く・閉じる。行き過ぎず、すぐ動き出して滑らかに止まる（ease-out。滑り込みと同じ長さ）。
    Grow,
}

impl Motion {
    pub const fn duration_ms(self) -> u64 {
        match self {
            Motion::Enter => duration_ms::NORMAL,
            Motion::Exit | Motion::Return => duration_ms::QUICK,
            Motion::SlideIn | Motion::SlideOut | Motion::Grow => SLIDE_MS,
            Motion::Move => duration_ms::NORMAL,
            Motion::Morph => duration_ms::SLOW,
        }
    }

    /// 進み具合 `p`（0..=1）をこの動きのイージングに通した値。長さは呼び出し側が持つ（色の移り変わりや波紋）。
    pub fn ease(self, p: f32) -> f32 {
        self.easing().apply(p)
    }

    fn easing(self) -> Easing {
        match self {
            Motion::Enter | Motion::Return | Motion::Grow => Easing::EaseOutCubic,
            Motion::Exit => Easing::EaseInCubic,
            Motion::Move | Motion::Morph => Easing::EaseInOutCubic,
            Motion::SlideIn => Easing::EaseOutBack,
            // 滑り込みを逆に再生した動き（ease-in-back は 1 - ease-out-back(1 - t)）。
            Motion::SlideOut => Easing::EaseInBack,
        }
    }
}

/// `from` から `to` へ [`Motion`] の長さで動く値。毎フレーム [`Animation::advance`] に経過秒を渡す。
#[derive(Debug)]
pub struct Animation(Tween<f32>);

impl Animation {
    pub fn new(from: f32, to: f32, motion: Motion) -> Self {
        Self(
            Tween::new(from, to)
                .duration(motion.duration_ms() as f32 / 1000.0)
                .easing(motion.easing())
                .build(),
        )
    }

    /// `dt` 秒進めて、今の値を返す。
    pub fn advance(&mut self, dt: f32) -> f32 {
        self.0.update(dt);
        self.0.value()
    }

    pub fn value(&self) -> f32 {
        self.0.value()
    }

    pub fn is_done(&self) -> bool {
        self.0.is_complete()
    }
}

/// `Animation` と違い毎フレーム進めず、始めた時刻と「今」から値を引く。呼び出し側は `Instant::now()` を渡すだけ。
/// 動いている間（`at` の 2 つ目が `true`）は描き続けること。
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    from: f32,
    to: f32,
    start: Option<(Instant, Motion)>,
}

impl Transition {
    pub const fn rest(v: f32) -> Self {
        Self {
            from: v,
            to: v,
            start: None,
        }
    }

    /// 今の値と、まだ動いているか。
    pub fn at(&self, now: Instant) -> (f32, bool) {
        let Some((start, motion)) = self.start else {
            return (self.to, false);
        };
        let p = now.saturating_duration_since(start).as_secs_f32() * 1000.0
            / motion.duration_ms() as f32;
        if p >= 1.0 {
            return (self.to, false);
        }
        (self.from + (self.to - self.from) * motion.ease(p), true)
    }

    /// 今の値から `to` へ `motion` で動き直す。行き先が同じなら何もしない。`animated` でなければ即座。
    pub fn go(&mut self, to: f32, motion: Motion, now: Instant, animated: bool) {
        if self.to == to {
            return;
        }
        let (cur, _) = self.at(now);
        *self = if animated && cur != to {
            Self {
                from: cur,
                to,
                start: Some((now, motion)),
            }
        } else {
            Self::rest(to)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_from_start_to_end() {
        let mut a = Animation::new(0.0, 1.0, Motion::Enter);
        assert_eq!(a.value(), 0.0);
        assert!(!a.is_done());
        let mid = a.advance(0.1);
        assert!(mid > 0.0 && mid < 1.0);
        assert_eq!(a.advance(1.0), 1.0);
        assert!(a.is_done());
    }

    #[test]
    fn slide_overshoots_both_ways_and_settles() {
        let (mut i, mut o) = (
            Animation::new(0.0, 1.0, Motion::SlideIn),
            Animation::new(1.0, 0.0, Motion::SlideOut),
        );
        let (mut max_in, mut max_out) = (0.0f32, 0.0f32);
        for _ in 0..100 {
            max_in = max_in.max(i.advance(0.01));
            max_out = max_out.max(o.advance(0.01));
        }
        assert!(max_in > 1.0, "入りは行き過ぎる: {max_in}");
        assert!(max_out > 1.0, "出は一度引く: {max_out}");
        assert_eq!((i.value(), o.value()), (1.0, 0.0));
    }

    #[test]
    fn grow_never_overshoots() {
        let (mut i, mut o) = (
            Animation::new(0.0, 1.0, Motion::Grow),
            Animation::new(1.0, 0.0, Motion::Grow),
        );
        for _ in 0..100 {
            let (a, b) = (i.advance(0.005), o.advance(0.005));
            assert!((0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b));
        }
        assert_eq!((i.value(), o.value()), (1.0, 0.0));
    }

    #[test]
    fn slide_takes_350_ms() {
        assert_eq!(Motion::SlideIn.duration_ms(), 350);
        assert_eq!(Motion::SlideOut.duration_ms(), 350);
        assert_eq!(Motion::Grow.duration_ms(), 350);
    }

    #[test]
    fn exit_is_faster_than_enter() {
        assert!(Motion::Exit.duration_ms() < Motion::Enter.duration_ms());
    }

    #[test]
    fn transition_eases_and_snaps_without_motion() {
        use std::time::Duration;
        let now = Instant::now();
        let mut t = Transition::rest(1.0);
        t.go(0.0, Motion::Enter, now, true);
        let (v, moving) = t.at(now + Duration::from_millis(50));
        assert!(moving && v > 0.0 && v < 1.0, "{v}");
        assert_eq!(t.at(now + Duration::from_secs(1)), (0.0, false));
        t.go(1.0, Motion::Return, now, false);
        assert_eq!(t.at(now), (1.0, false));
    }
}
