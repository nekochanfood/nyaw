//! アニメーション駆動。部品は `Instant::now()` を直接読まず、再描画も自ら要求せず、ここを経由する。
//! - [`now`]: アニメーションが参照する唯一の時計。テストは [`advance`] で進行させる（実際の待機なし）。
//! - [`keep_drawing`]: アニメーション中は次のフレームも描画する。VR のフレームループにも通知する（[`Motion`]）。

use std::{cell::Cell, time::Duration, time::Instant};

use gpui::{App, Window};

use crate::Motion;

thread_local! {
    static SKEW: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// アニメーション時計の現在時刻。
pub fn now() -> Instant {
    Instant::now() + SKEW.get()
}

/// 時計を `by` 進める（このスレッドのみ。テストが待機なしでアニメーションを完了させるために使用）。
pub fn advance(by: Duration) {
    SKEW.set(SKEW.get() + by);
}

/// アニメーション中は次のフレームも描画する。
pub fn keep_drawing(window: &mut Window, cx: &mut App) {
    if cx.has_global::<Motion>() {
        let id = window.window_handle().window_id();
        cx.global_mut::<Motion>().animating.insert(id);
    }
    window.request_animation_frame();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_moves_the_clock_forward() {
        let before = now();
        advance(Duration::from_secs(10));
        assert!(now() >= before + Duration::from_secs(10));
    }
}
