//! アニメーション。animato をここで包み、呼び出し側は [`Motion`] と [`Animation`] だけを見る。
//! 長さは `duration_ms`（tokens.json）、イージングは用途ごとに 1 つ決めてある。

use animato::{Easing, Tween, Update};

use crate::duration_ms;

/// ボタンの滑り込み・滑り出しの長さ（ms）。
const SLIDE_MS: u64 = 500;

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
    /// ボタンが滑り出る。少し逆に引いてから離れる（ease-in-back。滑り込みと同じ長さ）。
    SlideOut,
}

impl Motion {
    pub const fn duration_ms(self) -> u64 {
        match self {
            Motion::Enter => duration_ms::NORMAL,
            Motion::Exit | Motion::Return => duration_ms::QUICK,
            Motion::SlideIn | Motion::SlideOut => SLIDE_MS,
            Motion::Move => duration_ms::NORMAL,
            Motion::Morph => duration_ms::SLOW,
        }
    }

    fn easing(self) -> Easing {
        match self {
            Motion::Enter | Motion::Return => Easing::EaseOutCubic,
            Motion::Exit => Easing::EaseInCubic,
            Motion::Move | Motion::Morph => Easing::EaseInOutCubic,
            Motion::SlideIn => Easing::EaseOutBack,
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
    fn slide_takes_half_a_second() {
        assert_eq!(Motion::SlideIn.duration_ms(), 500);
        assert_eq!(Motion::SlideOut.duration_ms(), 500);
    }

    #[test]
    fn exit_is_faster_than_enter() {
        assert!(Motion::Exit.duration_ms() < Motion::Enter.duration_ms());
    }
}
