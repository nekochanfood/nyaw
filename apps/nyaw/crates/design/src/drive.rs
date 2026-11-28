//! アニメーション駆動。部品は `Instant::now()` を直接読まず、再描画も自ら要求せず、ここを経由する。
//!
//! アニメーションが参照する時計は [`now`] だけで、テストは [`advance`] で進める（実際の待機なし）。描き直しは、遷移の間は
//! [`keep_drawing`]、表示している間ずっと続く装飾のくり返しは [`keep_drawing_decor`]（[`DECOR_INTERVAL`] 以上空ける）、
//! くり返しの合間は [`draw_at`]、描画の外（非同期処理の完了）からは [`redraw`] で頼む。
//!
//! 要求は [`Motion`] で VR のフレームループに伝える。パネルのウィンドウは非表示で GPUI の次フレームの通知が届かないため、
//! フレームループが駆動している間は通知を要求しない（要求すると、実行されないまま溜まる）。

use std::{cell::Cell, time::Duration, time::Instant};

use gpui::{App, Window, WindowId};

use crate::Motion;

/// 装飾のくり返し（スピナー、波紋、案内の輪）を描き直す間隔の下限。フレーム単位に切り上がるので、表示周期 72〜144Hz では
/// 30〜45Hz になる。90Hz の 2 フレーム（22.2ms）が少し短く揺れても 3 フレームに延びないよう、22.2ms より短くする。
pub const DECOR_INTERVAL: Duration = Duration::from_micros(21_700);

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

/// 呼んだ時点から実時間で `wait` が過ぎるまで結果を返さないように `work` を包む。返す処理はバックグラウンドで実行する
/// （GPUI のタイマーは Windows では粗いので、std の sleep で待つ）。
pub fn not_before<T>(wait: Duration, work: impl FnOnce() -> T) -> impl FnOnce() -> T {
    let due = Instant::now() + wait;
    move || {
        let result = work();
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
        result
    }
}

impl Motion {
    /// フレームループが描き直しを駆動する場合の値。
    pub fn driven() -> Self {
        Self {
            frame: Some(now()),
            ..Default::default()
        }
    }

    /// フレームの始まりに、その時刻 `at` を渡して呼ぶ。要求の間隔の起点と、時刻が来たかの判定に使う。
    pub fn begin_frame(&mut self, at: Instant) {
        self.frame = Some(at);
    }

    /// `window` を描き直す時刻が来ていれば、要求を取り出して `true`。
    pub fn take_due(&mut self, window: WindowId) -> bool {
        let frame = self.frame();
        let due = self.animating.get(&window).is_some_and(|&at| at <= frame);
        if due {
            self.animating.remove(&window);
        }
        due
    }

    /// `window` を描く直前に呼ぶ。残っている要求は捨て、この描画で改めて出してもらう
    /// （残すと、入力などで描いた分に古い要求の分が重なり、装飾の頻度が上限を超える）。
    pub fn drawing(&mut self, window: WindowId) {
        self.animating.remove(&window);
    }

    fn frame(&self) -> Instant {
        self.frame.unwrap_or_else(now)
    }

    /// `window` を `at` に描き直す。複数の要求は早いほうを取る。
    fn wake(&mut self, window: WindowId, at: Instant) {
        let due = self.animating.entry(window).or_insert(at);
        *due = at.min(*due);
    }
}

/// 遷移の間は次のフレームも描画する。
pub fn keep_drawing(window: &mut Window, cx: &mut App) {
    request(window, cx, |frame| frame);
}

/// 装飾のくり返し（スピナーなど）の描き直し。頻度に上限がある（[`DECOR_INTERVAL`]）。
pub fn keep_drawing_decor(window: &mut Window, cx: &mut App) {
    request(window, cx, |frame| frame + DECOR_INTERVAL);
}

/// `at` まで描き直さなくてよい（動きの合間）。
pub fn draw_at(at: Instant, window: &mut Window, cx: &mut App) {
    request(window, cx, |_| at);
}

/// 描画の外から `window` を次のフレームで描き直す（バックグラウンドで作った画像の差し替えなど）。ビューの notify と併せて呼ぶ。
pub fn redraw(window: WindowId, cx: &mut App) {
    if cx.has_global::<Motion>() {
        let motion = cx.global_mut::<Motion>();
        let at = motion.frame();
        motion.wake(window, at);
    }
}

fn request(window: &mut Window, cx: &mut App, due: impl FnOnce(Instant) -> Instant) {
    if cx.has_global::<Motion>() {
        let id = window.window_handle().window_id();
        let motion = cx.global_mut::<Motion>();
        let at = due(motion.frame());
        motion.wake(id, at);
        if motion.frame.is_some() {
            return;
        }
    }
    // フレームループの無いウィンドウ（部品のカタログ、テスト）は、GPUI の次フレームで描き直す。
    window.request_animation_frame();
}

#[cfg(test)]
mod tests {
    use gpui::{Context, IntoElement, Render, TestAppContext, canvas};

    use super::*;

    #[test]
    fn advance_moves_the_clock_forward() {
        let before = now();
        advance(Duration::from_secs(10));
        assert!(now() >= before + Duration::from_secs(10));
    }

    #[test]
    fn not_before_holds_the_result_until_the_time_has_passed() {
        let wait = Duration::from_millis(20);
        let start = Instant::now();
        let work = not_before(wait, || 7);
        assert_eq!(work(), 7);
        assert!(start.elapsed() >= wait);
    }

    /// 表示周期 `hz`（揺れ ±0.4ms）のフレームを 10 秒進め、要求を出し続けたウィンドウを描いた毎秒の回数。
    fn draws_per_second(hz: f64, interval: Duration) -> f64 {
        let window = WindowId::default();
        let start = Instant::now();
        let mut motion = Motion::driven();
        motion.wake(window, start);
        let frames = (hz * 10.0) as u32;
        let mut draws = 0;
        for n in 0..frames {
            let jitter = if n % 3 == 0 { 0.0004 } else { -0.0002 };
            let frame = start + Duration::from_secs_f64(f64::from(n) / hz + 0.0004 + jitter);
            motion.begin_frame(frame);
            if motion.take_due(window) {
                draws += 1;
                motion.drawing(window);
                motion.wake(window, frame + interval);
            }
        }
        f64::from(draws) / 10.0
    }

    #[test]
    fn decor_redraws_between_30_and_45_hz_on_every_headset_rate() {
        for hz in [72.0, 80.0, 90.0, 120.0, 144.0] {
            let rate = draws_per_second(hz, DECOR_INTERVAL);
            assert!((30.0..=45.5).contains(&rate), "{hz}Hz: {rate}");
        }
    }

    #[test]
    fn transitions_redraw_every_frame() {
        for hz in [72.0, 90.0, 144.0] {
            let rate = draws_per_second(hz, Duration::ZERO);
            assert!((rate - hz).abs() < 0.2, "{hz}Hz: {rate}");
        }
    }

    #[test]
    fn the_earliest_request_wins_and_waits_for_its_time() {
        let window = WindowId::default();
        let start = Instant::now();
        let mut motion = Motion::driven();
        motion.begin_frame(start);
        motion.wake(window, start + Duration::from_secs(2));
        motion.wake(window, start + Duration::from_secs(1));
        motion.wake(window, start + Duration::from_secs(3));
        motion.begin_frame(start + Duration::from_millis(999));
        assert!(!motion.take_due(window));
        motion.begin_frame(start + Duration::from_secs(1));
        assert!(motion.take_due(window));
        assert!(!motion.take_due(window), "taken once");
    }

    struct Decor;
    impl Render for Decor {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            canvas(
                |_, _, _| (),
                |_, _, window, cx| {
                    keep_drawing(window, cx);
                    keep_drawing_decor(window, cx);
                    draw_at(now() + Duration::from_secs(1), window, cx);
                },
            )
        }
    }

    /// 描画のあとに残っている、GPUI の次フレームの通知の数。
    fn next_frame_callbacks(motion: Motion, cx: &mut TestAppContext) -> usize {
        cx.update(|cx| cx.set_global(motion));
        let (_, cx) = cx.add_window_view(|_, _| Decor);
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx)
        })
    }

    #[gpui::test]
    fn a_frame_loop_gets_no_next_frame_callbacks(cx: &mut TestAppContext) {
        assert_eq!(next_frame_callbacks(Motion::driven(), cx), 0);
    }

    #[gpui::test]
    fn without_a_frame_loop_gpui_draws_the_next_frame(cx: &mut TestAppContext) {
        assert!(next_frame_callbacks(Motion::default(), cx) > 0);
    }
}
