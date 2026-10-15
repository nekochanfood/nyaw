//! アニメーション。animato をここで包み、呼び出し側は [`Motion`] と [`Animation`] だけを見る。
//! 長さは `duration_ms`（tokens.json）、イージングは用途ごとに 1 つ決めてある。

use animato::{Easing, Tween, Update};

use crate::duration_ms;

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
}

impl Motion {
    pub const fn duration_ms(self) -> u64 {
        match self {
            Motion::Enter => duration_ms::NORMAL,
            Motion::Exit | Motion::Return => duration_ms::QUICK,
            Motion::Move => duration_ms::NORMAL,
        }
    }

    fn easing(self) -> Easing {
        match self {
            Motion::Enter | Motion::Return => Easing::EaseOutCubic,
            Motion::Exit => Easing::EaseInCubic,
            Motion::Move => Easing::EaseInOutCubic,
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
    fn exit_is_faster_than_enter() {
        assert!(Motion::Exit.duration_ms() < Motion::Enter.duration_ms());
    }
}
