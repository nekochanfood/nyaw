//! アニメーション。animato をここで包み、呼び出し側は [`Motion`] と [`Animation`] だけを見る。
//! 長さは `duration_ms`（tokens.json）、イージングは用途ごとに 1 つ決めてある。

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
    /// ボタンが滑り込む。鋭く立ち上がって行き過ぎ、収まる（ease-out-back より強い cubic-bezier）。
    SlideIn,
    /// ボタンが滑り出る。大きく逆に引いてから加速して離れる（ease-in-back より強い cubic-bezier。滑り込みと同じ長さ）。
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

    fn easing(self) -> Easing {
        match self {
            Motion::Enter | Motion::Return | Motion::Grow => Easing::EaseOutCubic,
            Motion::Exit => Easing::EaseInCubic,
            Motion::Move | Motion::Morph => Easing::EaseInOutCubic,
            // 立ち上がりが鋭く、2 割ほど行き過ぎてから収まる（標準の ease-out-back は 1 割ほどで、短い間だと直線に見える）。
            Motion::SlideIn => Easing::CubicBezier(0.2, 1.9, 0.3, 1.0),
            // 一度大きく引いてから、加速して離れる。
            Motion::SlideOut => Easing::CubicBezier(0.7, -0.7, 0.8, 0.0),
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
}
