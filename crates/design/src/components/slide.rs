//! ボタンの滑り込み・滑り出し。ダッシュボードのバーと、その動きを試すテスト画面（`testing`）が共有する。

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui::{Div, SharedString, div, prelude::*, px};

use super::BUTTON_HEIGHT;
use crate::spacing;

/// ボタンを 1 つずつ順に出すときの間隔。
pub const STAGGER: Duration = Duration::from_millis(40);
/// これだけ描かれなかったら、隠れていた（出し直した）とみなす。
const IDLE_GAP: Duration = Duration::from_secs(3);

/// ボタンの出方: 下から上へ滑り込む。ダッシュボードを出すときは、左のボタンから [`STAGGER`] ずつ遅れて順に。
/// ボタンが足されたときは、それに加えて幅が開いて隣を押し広げる（バーは中身の幅に収まるので、幅も滑らかに変わる）。
/// 消えるボタンは、下へ滑り出ながら幅を閉じて隣が詰まる（入りの逆）。どちらも少し行き過ぎてから収まる。
/// また足されたらもう一度滑り込む。
#[derive(Default)]
pub struct Entering {
    seen: HashSet<SharedString>,
    /// 動いているボタン: 出具合・始まる時刻（遅れて始めるものは未来）・幅も開くか。
    running: HashMap<SharedString, (crate::animation::Animation, Instant, bool)>,
    /// このフレームに描いたボタン。
    touched: HashSet<SharedString>,
    /// 消えていくボタン: 出具合（1 から 0 へ）と、前に進めた時刻。
    leaving: HashMap<SharedString, (crate::animation::Animation, Instant)>,
    /// 最後に描いた時刻と、隠れていたか。
    last: Option<Instant>,
    hid: bool,
    /// このフレームは、ダッシュボードを出した直後（全部のボタンが順に出る）。
    revealing: bool,
    /// 動かさない（出きった状態で描く）。画面のテストが、動いている途中のボタンを押さないように。
    off: bool,
}

impl Entering {
    /// `off` なら動かさない（出きった状態で描く）。
    pub fn new(off: bool) -> Self {
        Self {
            off,
            ..Self::default()
        }
    }

    /// 全部のボタンを、次のフレームから左のものから順に出し直す（ダッシュボードを出し直したときと同じ動き）。
    pub fn replay(&mut self) {
        self.hid = true;
    }

    /// フレームの初めに呼ぶ。隠れていた（または初めての）ダッシュボードが出たなら、全部のボタンを出し直す。
    pub fn begin(&mut self, hidden: bool, now: Instant) {
        if hidden {
            self.hid = true;
        } else if self.hid
            || self
                .last
                .is_none_or(|l| now.saturating_duration_since(l) > IDLE_GAP)
        {
            (self.hid, self.revealing) = (false, true);
            self.seen.clear();
            self.running.clear();
            self.leaving.clear();
        }
        self.last = Some(now);
    }

    /// このフレームの `key` のボタン（左から数えて `slot` 番目）の出具合（0..=1。1 で出きっている）と、幅も開くか。
    pub fn progress(&mut self, key: &SharedString, slot: usize, now: Instant) -> (f32, bool) {
        if self.off {
            return (1.0, false);
        }
        self.touched.insert(key.clone());
        self.leaving.remove(key);
        if self.seen.insert(key.clone()) {
            let start = if self.revealing {
                now + STAGGER * slot as u32
            } else {
                now
            };
            let enter =
                crate::animation::Animation::new(0.0, 1.0, crate::animation::Motion::SlideIn);
            self.running
                .insert(key.clone(), (enter, start, !self.revealing));
        }
        let Some((anim, last, grow)) = self.running.get_mut(key) else {
            return (1.0, false);
        };
        let grow = *grow;
        if now < *last {
            return (0.0, grow);
        }
        let t = anim.advance(now.duration_since(*last).as_secs_f32());
        *last = now;
        if anim.is_done() {
            self.running.remove(key);
        }
        (t, grow)
    }

    /// 出ていた `key` のボタンが無くなった。消えていく間（まだ描き残すか）なら `true`。
    pub fn start_exit(&mut self, key: &SharedString, now: Instant) -> bool {
        if self.leaving.contains_key(key) {
            return true;
        }
        if !self.seen.remove(key) {
            return false;
        }
        self.running.remove(key);
        let exit = crate::animation::Animation::new(1.0, 0.0, crate::animation::Motion::SlideOut);
        self.leaving.insert(key.clone(), (exit, now));
        true
    }

    /// 消えていくボタンの出具合（1 から 0 へ。終われば忘れる）。
    pub fn progress_out(&mut self, key: &SharedString, now: Instant) -> f32 {
        let Some((anim, last)) = self.leaving.get_mut(key) else {
            return 0.0;
        };
        let t = anim.advance(now.saturating_duration_since(*last).as_secs_f32());
        *last = now;
        if anim.is_done() {
            self.leaving.remove(key);
        }
        t
    }

    /// 1 フレーム描き終えた。描かなかったボタンは忘れる。動いているものがあるか（次のフレームも描くか）を返す。
    pub fn finish(&mut self) -> bool {
        let touched = std::mem::take(&mut self.touched);
        self.seen.retain(|k| touched.contains(k));
        self.running.retain(|k, _| touched.contains(k));
        self.revealing = false;
        !self.running.is_empty() || !self.leaving.is_empty()
    }
}

/// `cur`（今ある一覧）に、前のフレームにあって無くなったもの（`prev`）を、前の場所に挟んで返す。
/// 挟んだもの（消えていく間のもの）には `true` が付く。
pub fn with_leaving<T: Clone>(
    cur: Vec<T>,
    prev: &[T],
    key: impl Fn(&T) -> SharedString,
    enter: &mut Entering,
    now: Instant,
) -> Vec<(T, bool)> {
    let keys: HashSet<_> = cur.iter().map(&key).collect();
    let mut out: Vec<_> = cur.into_iter().map(|t| (t, false)).collect();
    for (i, p) in prev.iter().enumerate() {
        if keys.contains(&key(p)) || !enter.start_exit(&key(p), now) {
            continue;
        }
        // 前の並びで手前にあったもののうち、今も並んでいる一番近いものの後ろ。
        let at = prev[..i]
            .iter()
            .rev()
            .find_map(|q| out.iter().position(|(o, _)| key(o) == key(q)))
            .map_or(0, |j| j + 1);
        out.insert(at, (p.clone(), true));
    }
    out
}

/// 縦に行き過ぎた分が切られないよう、箱の上下に取る余白（px。余白の分は負の余白で打ち消す）。
const OVERSHOOT_PAD: f32 = 12.0;

/// 出具合 `t`（0 から 1、行き過ぎれば少し超える）のボタンを、下から上へ滑り込ませる（`t` が減れば逆に滑り出る）。`grow` なら幅も `t` に絞った箱に入れ、
/// 隣との隙間も `t` に比例して開く（負の余白で、残りの隙間を打ち消す）。
pub fn slide_in(t: f32, grow: bool, child: impl IntoElement) -> Div {
    let h = BUTTON_HEIGHT;
    div()
        .flex_none()
        .overflow_hidden()
        .py(px(OVERSHOOT_PAD))
        .my(px(-OVERSHOOT_PAD))
        .when(grow, |d| {
            d.w(px(h * t.max(0.0))).mr(px(-spacing::SM * (1.0 - t)))
        })
        .child(
            div()
                .relative()
                .top(px(h * (1.0 - t)))
                .opacity(t.clamp(0.0, 1.0))
                .child(child),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> SharedString {
        s.to_owned().into()
    }

    /// 出したときは左のボタンから順に滑り込み（幅は変えない）、全部出きったら止まる。
    #[test]
    fn buttons_slide_in_from_the_left_when_shown() {
        let (mut e, t0) = (Entering::default(), Instant::now());
        e.begin(false, t0);
        let (a, b) = (key("a"), key("b"));
        assert_eq!(e.progress(&a, 0, t0), (0.0, false));
        assert_eq!(e.progress(&b, 3, t0), (0.0, false));
        assert!(e.finish());
        let later = t0 + STAGGER * 2;
        e.begin(false, later);
        let ((ta, _), (tb, _)) = (e.progress(&a, 0, later), e.progress(&b, 3, later));
        assert!(ta > 0.0 && tb == 0.0, "左が先: {ta} / {tb}");
        e.finish();
        let end = t0 + Duration::from_secs(2);
        e.begin(false, end);
        assert_eq!(e.progress(&a, 0, end), (1.0, false));
        assert_eq!(e.progress(&b, 3, end), (1.0, false));
        assert!(!e.finish(), "出きったら次のフレームは要らない");
    }

    /// 足されたボタンだけが、幅も開きながら滑り込む。消えて戻ってくればもう一度。
    #[test]
    fn added_button_grows_while_the_others_stay() {
        let (mut e, t0) = (Entering::default(), Instant::now());
        let done = t0 + Duration::from_secs(2);
        e.begin(false, t0);
        e.progress(&key("a"), 0, t0);
        e.finish();
        e.begin(false, done);
        e.progress(&key("a"), 0, done);
        e.finish();
        e.begin(false, done);
        assert_eq!(e.progress(&key("a"), 0, done), (1.0, false));
        assert_eq!(e.progress(&key("b"), 1, done), (0.0, true));
        e.finish();
        let mid = done + Duration::from_millis(50);
        e.begin(false, mid);
        let (t, grow) = e.progress(&key("b"), 1, mid);
        assert!(t > 0.0 && t < 1.0 && grow, "途中: {t}");
        e.finish();
        // 消えて、また足された。
        e.begin(false, mid);
        e.progress(&key("a"), 0, mid);
        e.finish();
        e.begin(false, mid);
        assert_eq!(e.progress(&key("b"), 1, mid), (0.0, true));
    }

    /// 隠れていたダッシュボードを出し直すと、出ていたボタンも順に出し直す。
    #[test]
    fn showing_again_replays_the_slide() {
        let (mut e, t0) = (Entering::default(), Instant::now());
        let done = t0 + Duration::from_secs(2);
        e.begin(false, t0);
        e.progress(&key("a"), 0, t0);
        e.finish();
        e.begin(false, done);
        assert_eq!(e.progress(&key("a"), 0, done).0, 1.0);
        e.finish();
        e.begin(true, done);
        e.progress(&key("a"), 0, done);
        e.finish();
        e.begin(false, done);
        assert_eq!(e.progress(&key("a"), 0, done), (0.0, false));
    }

    /// 消えるボタンは、下へ滑り出ながら（幅も閉じて）終われば忘れる。前の場所に描き残す。
    #[test]
    fn removed_button_slides_out_in_its_old_place() {
        let (mut e, t0) = (Entering::default(), Instant::now());
        let done = t0 + Duration::from_secs(2);
        let keys = [key("a"), key("b"), key("c")];
        for now in [t0, done] {
            e.begin(false, now);
            for (i, k) in keys.iter().enumerate() {
                e.progress(k, i, now);
            }
            e.finish();
        }
        // 真ん中の b が消えた: 前の場所に、消えていくものとして残る。
        let (cur, prev) = (vec!["a", "c"], vec!["a", "b", "c"]);
        let items = with_leaving(cur, &prev, |k| key(k), &mut e, done);
        assert_eq!(items, [("a", false), ("b", true), ("c", false)]);
        let mid = done + Duration::from_millis(100);
        let out = e.progress_out(&key("b"), mid);
        assert!(out > 0.0, "途中: {out}");
        e.begin(false, mid);
        e.progress(&key("a"), 0, mid);
        e.progress(&key("c"), 2, mid);
        assert!(e.finish(), "消えている間は次のフレームも描く");
        // 終われば、並びから外れる。
        let end = done + Duration::from_secs(2);
        assert_eq!(e.progress_out(&key("b"), end), 0.0);
        e.begin(false, end);
        let items = with_leaving(vec!["a", "c"], &prev, |k| key(k), &mut e, end);
        assert_eq!(items, [("a", false), ("c", false)]);
        assert!(!e.finish());
    }
}
