//! 動きの駆動。部品は時刻を `Instant::now()` で読まず、再描画も自分で要求せず、ここを通す。
//! - [`now`]: 動きが見る唯一の時計。テストは [`advance`] で進められる（実際に待たない）。
//! - [`keep_drawing`]: 動いている間は次のフレームも描く。VR のフレームループにも伝える（[`Motion`]）。

use std::{cell::Cell, time::Duration, time::Instant};

use gpui::{App, Window};

use crate::Motion;

thread_local! {
    static SKEW: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// 動きの時計の「今」。
pub fn now() -> Instant {
    Instant::now() + SKEW.get()
}

/// 時計を `by` 進める（このスレッドの動きだけ。テストが待たずに動きを終わらせるために使う）。
pub fn advance(by: Duration) {
    SKEW.set(SKEW.get() + by);
}

/// 動いている間は次のフレームも描く。
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
