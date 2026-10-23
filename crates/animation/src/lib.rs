//! アニメーション。animato をここで包み、呼び出し側は [`Motion`] と [`Animation`] だけを見る。
//! 長さは `duration_ms`（tokens.json）、イージングは用途ごとに 1 つ決めてある。

use std::time::{Duration, Instant};

use animato::{Easing, Tween, Update};

include!(concat!(env!("OUT_DIR"), "/duration_ms.rs"));

/// ボタンの滑り込み・滑り出しの長さ（ms）。
const SLIDE_MS: u64 = 350;

/// 幅の動きと中身の動きのずれ（入りは幅が先、出は中身が先）。
pub const LAG: Duration = Duration::from_millis(80);

/// ボタンの出具合。`rise`（縦のずれと透明度。行き過ぎる）と `grow`（幅。行き過ぎない）は別々に動く。1 で出きっている。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    pub rise: f32,
    pub grow: f32,
}

impl Slide {
    pub const GONE: Self = Self {
        rise: 0.0,
        grow: 0.0,
    };
    pub const SHOWN: Self = Self {
        rise: 1.0,
        grow: 1.0,
    };
}

/// 縦の動き（行き過ぎる）と幅の動き（行き過ぎない）の組。ボタンの滑り込み・滑り出しに使う。
pub struct Pair {
    rise: Animation,
    grow: Animation,
    /// まだ動き出していない側（入りなら `rise`、出なら `grow`）があと何秒待つか。
    wait: f32,
    /// 待っているのは `rise` か（そうでなければ `grow`）。
    rise_waits: bool,
}

impl Pair {
    /// 入り（`entering`）は幅が先に開き始め、少し遅れて中身が出る。出は中身が先に消え始め、少し遅れて幅が閉じる。
    pub fn new(from: f32, to: f32, rise: Motion, entering: bool) -> Self {
        Self {
            rise: Animation::new(from, to, rise),
            grow: Animation::new(
                from,
                to,
                if entering {
                    Motion::Grow
                } else {
                    Motion::Shrink
                },
            ),
            wait: LAG.as_secs_f32(),
            rise_waits: entering,
        }
    }

    pub fn advance(&mut self, dt: f32) -> Slide {
        let late = dt.min(self.wait);
        self.wait -= late;
        let (rise_dt, grow_dt) = if self.rise_waits {
            (dt - late, dt)
        } else {
            (dt, dt - late)
        };
        Slide {
            rise: self.rise.advance(rise_dt),
            grow: self.grow.advance(grow_dt),
        }
    }

    pub fn is_done(&self) -> bool {
        self.rise.is_done() && self.grow.is_done()
    }
}

/// 動きの用途。長さとイージングはここで決まる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// 現れる。ゆっくり着地する（ease-out）。
    Enter,
    /// 消える。現れるの逆再生（同じ長さで ease-in）。
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
    /// ボタンの幅が閉じる。開くの逆再生（ease-in。同じ長さ）。
    Shrink,
}

impl Motion {
    pub const fn duration_ms(self) -> u64 {
        match self {
            Motion::Return => duration_ms::QUICK,
            Motion::Enter | Motion::Exit | Motion::Move => duration_ms::NORMAL,
            Motion::SlideIn | Motion::SlideOut | Motion::Grow | Motion::Shrink => SLIDE_MS,
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
            Motion::Exit | Motion::Shrink => Easing::EaseInCubic,
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
            Animation::new(1.0, 0.0, Motion::Shrink),
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

    /// 出は入りの逆再生: 同じ長さで、時間を裏返した値になる。
    #[test]
    fn out_is_in_played_backwards() {
        for (i, o) in [
            (Motion::Enter, Motion::Exit),
            (Motion::SlideIn, Motion::SlideOut),
            (Motion::Grow, Motion::Shrink),
        ] {
            assert_eq!(i.duration_ms(), o.duration_ms());
            for k in 0..=10 {
                let t = k as f32 / 10.0;
                assert!(
                    (o.ease(t) - (1.0 - i.ease(1.0 - t))).abs() < 1e-5,
                    "{i:?} {t}"
                );
            }
        }
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

    /// 入りは幅が先に開いてから中身が出て、出は中身が先に消えてから幅が閉じる。
    #[test]
    fn width_leads_when_entering_and_trails_when_leaving() {
        let early = LAG.as_secs_f32() / 2.0;
        let mut enter = Pair::new(0.0, 1.0, Motion::SlideIn, true);
        let s = enter.advance(early);
        assert!(s.grow > 0.0 && s.rise == 0.0, "{s:?}");
        let mut leave = Pair::new(1.0, 0.0, Motion::SlideOut, false);
        let s = leave.advance(early);
        assert!(s.grow == 1.0 && s.rise != 1.0, "{s:?}");
    }
}
