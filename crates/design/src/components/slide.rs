//! ボタンの滑り込み・滑り出し。ダッシュボードのバーと、その動きを試すテスト画面（`testing`）が共有する。

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui::{Div, ScrollHandle, SharedString, div, prelude::*, px};

use super::BUTTON_HEIGHT;
use crate::animation::{Animation, Motion};
use crate::{Theme, radius, spacing};

/// ボタンを 1 つずつ順に出すときの間隔。
pub const STAGGER: Duration = Duration::from_millis(60);
/// 幅の動きと中身の動きのずれ（入りは幅が先、出は中身が先）。
const LAG: Duration = Duration::from_millis(80);
/// これだけ描かれなかったら、隠れていた（出し直した）とみなす。
const IDLE_GAP: Duration = Duration::from_secs(3);

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

/// 縦の動き（行き過ぎる）と幅の動き（行き過ぎない）の組。
struct Pair {
    rise: Animation,
    grow: Animation,
    /// まだ動き出していない側（入りなら `rise`、出なら `grow`）があと何秒待つか。
    wait: f32,
    /// 待っているのは `rise` か（そうでなければ `grow`）。
    rise_waits: bool,
}

impl Pair {
    /// 入り（`entering`）は幅が先に開き始め、少し遅れて中身が出る。出は中身が先に消え始め、少し遅れて幅が閉じる。
    fn new(from: f32, to: f32, rise: Motion, entering: bool) -> Self {
        Self {
            rise: Animation::new(from, to, rise),
            grow: Animation::new(from, to, Motion::Grow),
            wait: LAG.as_secs_f32(),
            rise_waits: entering,
        }
    }

    fn advance(&mut self, dt: f32) -> Slide {
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

    fn is_done(&self) -> bool {
        self.rise.is_done() && self.grow.is_done()
    }
}

/// ボタンの出方: 下から上へ滑り込む。ダッシュボードを出すときは、左のボタンから [`STAGGER`] ずつ遅れて順に。
/// ボタンが足されたときは、それに加えて幅が開いて隣を押し広げる（バーは中身の幅に収まるので、幅も滑らかに変わる）。
/// 消えるボタンは、下へ滑り出ながら幅を閉じて隣が詰まる（入りの逆）。どちらも少し行き過ぎてから収まる。
/// また足されたらもう一度滑り込む。
#[derive(Default)]
pub struct Entering {
    seen: HashSet<SharedString>,
    /// 動いているボタン: 出具合・始まる時刻（遅れて始めるものは未来）。
    running: HashMap<SharedString, (Pair, Instant)>,
    /// このフレームに描いたボタン。
    touched: HashSet<SharedString>,
    /// 消えていくボタン: 出具合（1 から 0 へ）と、前に進めた時刻。
    leaving: HashMap<SharedString, (Pair, Instant)>,
    /// 最後に描いた時刻と、隠れていたか。
    last: Option<Instant>,
    hid: bool,
    /// このフレームは、ダッシュボードを出した直後（全部のボタンが順に出る）。
    revealing: bool,
    /// このフレームに出ていたボタンのうち一番開いたものの出具合と、前のフレームの分（[`Entering::peak`]）。
    frame_max: f32,
    peak: f32,
    /// 足されたばかりのボタンが開き終わるまで、それを追いかけてスクロールするための印: そのボタンと、このフレームの位置。
    follow: Option<SharedString>,
    fresh: Option<usize>,
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

    /// このフレームの `key` のボタン（左から数えて `slot` 番目。バーの子の番号と同じ）の出具合。
    pub fn progress(&mut self, key: &SharedString, slot: usize, now: Instant) -> Slide {
        let s = self.step(key, slot, now);
        self.frame_max = self.frame_max.max(s.grow.clamp(0.0, 1.0));
        s
    }

    fn step(&mut self, key: &SharedString, slot: usize, now: Instant) -> Slide {
        if self.off {
            return Slide::SHOWN;
        }
        self.touched.insert(key.clone());
        self.leaving.remove(key);
        if self.seen.insert(key.clone()) {
            let start = if self.revealing {
                now + STAGGER * slot as u32
            } else {
                now
            };
            self.running.insert(
                key.clone(),
                (Pair::new(0.0, 1.0, Motion::SlideIn, true), start),
            );
            if !self.revealing {
                self.follow = Some(key.clone());
            }
        }
        if self.follow.as_ref() == Some(key) {
            self.fresh = Some(slot);
        }
        let Some((anim, last)) = self.running.get_mut(key) else {
            if self.follow.as_ref() == Some(key) {
                self.follow = None;
            }
            return Slide::SHOWN;
        };
        if now < *last {
            return Slide::GONE;
        }
        let t = anim.advance(now.duration_since(*last).as_secs_f32());
        *last = now;
        if anim.is_done() {
            self.running.remove(key);
        }
        t
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
        self.leaving.insert(
            key.clone(),
            (Pair::new(1.0, 0.0, Motion::SlideOut, false), now),
        );
        true
    }

    /// 消えていくボタンの出具合（1 から 0 へ。終われば忘れる）。
    pub fn progress_out(&mut self, key: &SharedString, now: Instant) -> Slide {
        let s = self.step_out(key, now);
        self.frame_max = self.frame_max.max(s.grow.clamp(0.0, 1.0));
        s
    }

    fn step_out(&mut self, key: &SharedString, now: Instant) -> Slide {
        let Some((anim, last)) = self.leaving.get_mut(key) else {
            return Slide::GONE;
        };
        let t = anim.advance(now.saturating_duration_since(*last).as_secs_f32());
        *last = now;
        if anim.is_done() {
            self.leaving.remove(key);
        }
        t
    }

    /// 足されたばかりのボタンの位置（`progress` に渡した `slot`）。開き終わるまで毎フレーム返すので、呼ぶ側はそのたびに
    /// `ScrollHandle::scroll_to_item` でそこまでスクロールする（開く途中は幅が小さいので、追いかける）。
    pub fn take_fresh(&mut self) -> Option<usize> {
        self.fresh.take()
    }

    /// 直前の [`Entering::finish`] までのフレームで、一番開いていたボタンの出具合（0 から 1）。
    /// ボタンが 1 つも開いていないうちは 0 で、カプセル自体も開かない（`fit_bar` の `grow`）。
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// 1 フレーム描き終えた。描かなかったボタンは忘れる。動いているものがあるか（次のフレームも描くか）を返す。
    pub fn finish(&mut self) -> bool {
        self.peak = std::mem::take(&mut self.frame_max);
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
const OVERSHOOT_PAD: f32 = 8.0;

/// 出具合 `s` のボタン（自然な幅 `width` px）を、下から上へ滑り込ませる（`s.rise` が減れば逆に滑り出る。少し行き過ぎてもよい）。
/// 幅も `s.grow` に比例して開き（隣との隙間 [`spacing::SM`] も一緒に。隙間はボタンの右に付く余白で、バーの `gap` は使わない）、
/// バーの幅が滑らかに変わる。
/// 幅は行き過ぎさせない（行き過ぎると、バー全体が広がって縮み、ガクつく）。
pub fn slide_in(s: Slide, width: f32, child: impl IntoElement) -> Div {
    reveal(s, width, true, child)
}

/// [`slide_in`] の、滑らず透明度と幅だけが変わる版（区切り線など）。
pub fn fade_in(s: Slide, width: f32, child: impl IntoElement) -> Div {
    reveal(s, width, false, child)
}

fn reveal(s: Slide, width: f32, rise: bool, child: impl IntoElement) -> Div {
    let w = s.grow.clamp(0.0, 1.0);
    div()
        .flex_none()
        .overflow_hidden()
        .py(px(OVERSHOOT_PAD))
        .my(px(-OVERSHOOT_PAD))
        // 出きったら自然な幅に任せる（中身の幅が変わっても追従する）。
        .when(w < 1.0, |d| d.w(px((width + spacing::SM) * w)))
        .child(
            div()
                .relative()
                .flex_none()
                .w(px(width))
                // 隣との隙間。出きったら自然な幅の一部になり、開く間は枠が狭くて切られる。
                .mr(px(spacing::SM))
                .when(rise, |d| d.top(px(BUTTON_HEIGHT * (1.0 - s.rise))))
                // 透明度は縦のずれと同じ動き（入りは幅より [`LAG`] 遅れて、出は幅より [`LAG`] 先に）。
                // 枠がまだ狭いうちは、ボタンの地が細い四角（円の端の切れ端）に見えるので出さない。
                .opacity(s.rise.clamp(0.0, 1.0) * ((w - 0.4) / 0.6).clamp(0.0, 1.0))
                .child(child),
        )
}

/// 中身の幅に収まるカプセルのバー。`grow` はボタンの開き具合（[`Entering::peak`]。開ききったら 1）で、開く前は小さく始まる。中身が `max_width`（px。任意だが、ほぼ指定する）を超えそうなら、そこで頭打ちにして
/// 横にスクロールでき、スクロールバーが出る。`id` はスクロール要素の id（棒は `{id}-bars-h`）。
/// 置き場所（中央寄せなど）は呼ぶ側が決める。
pub fn fit_bar(
    theme: &Theme,
    id: &'static str,
    scroll: &ScrollHandle,
    grow: f32,
    max_width: Option<f32>,
    children: impl IntoIterator<Item = impl IntoElement>,
) -> Div {
    // 縦に行き過ぎた分が切られないよう、スクロールの箱も上下に余白を取る。
    let body = div()
        .id(id)
        .track_scroll(scroll)
        .min_w_0()
        .flex()
        .items_center()
        // 左の余白（右の余白は、末尾のボタンの右の隙間）。棒の長さの基準になる箱の中に取る。
        .pl(px(spacing::SM * grow))
        // ボタンの枠（上下に余白つき）がちょうど収まる高さ。縦にはあふれない（縦の棒の分を横の棒が譲らない）。
        .h(px(BUTTON_HEIGHT + 2.0 * OVERSHOOT_PAD))
        .my(px(-OVERSHOOT_PAD))
        .overflow_x_scroll()
        .children(children);
    let bar = super::card_panel_fit(theme)
        .rounded(px(radius::FULL))
        .relative()
        .flex()
        .items_center()
        // ナビゲーションバーと同じ高さ（ボタンの上下に余白）にする。
        .py(px((super::NAV_BAR_HEIGHT - BUTTON_HEIGHT) / 2.0))
        // ボタンが開ききる前（`grow` < 1）は、カプセルも中身に合わせて低く、狭く始まる。
        .self_center()
        .overflow_hidden();
    let bar = if grow < 1.0 {
        bar.h(px(super::NAV_BAR_HEIGHT * grow.max(0.0)))
    } else {
        bar
    };
    let bar = match max_width {
        Some(w) => bar.max_w(px(w)),
        None => bar.max_w_full(),
    };
    bar.child(body).child(
        super::scrollbars(theme, format!("{id}-bars"), scroll)
            .corner_radius(super::NAV_BAR_HEIGHT / 2.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> SharedString {
        s.to_owned().into()
    }

    /// 出したときは左のボタンから順に滑り込み、全部出きったら止まる。
    #[test]
    fn buttons_slide_in_from_the_left_when_shown() {
        let (mut e, t0) = (Entering::default(), Instant::now());
        e.begin(false, t0);
        let (a, b) = (key("a"), key("b"));
        assert_eq!(e.progress(&a, 0, t0).rise, 0.0);
        assert_eq!(e.progress(&b, 3, t0).rise, 0.0);
        assert!(e.finish());
        let later = t0 + STAGGER * 2;
        e.begin(false, later);
        let (ta, tb) = (e.progress(&a, 0, later).rise, e.progress(&b, 3, later).rise);
        assert!(ta > 0.0 && tb == 0.0, "左が先: {ta} / {tb}");
        e.finish();
        let end = t0 + Duration::from_secs(2);
        e.begin(false, end);
        assert_eq!(e.progress(&a, 0, end).rise, 1.0);
        assert_eq!(e.progress(&b, 3, end).rise, 1.0);
        assert!(!e.finish(), "出きったら次のフレームは要らない");
    }

    /// 足されたボタンだけが滑り込む。消えて戻ってくればもう一度。
    #[test]
    fn added_button_grows_while_the_others_stay() {
        let (mut e, t0) = (Entering::default(), Instant::now());
        let done = t0 + Duration::from_secs(2);
        e.begin(false, t0);
        e.progress(&key("a"), 0, t0).rise;
        e.finish();
        e.begin(false, done);
        e.progress(&key("a"), 0, done).rise;
        e.finish();
        e.begin(false, done);
        assert_eq!(e.progress(&key("a"), 0, done).rise, 1.0);
        assert_eq!(e.progress(&key("b"), 1, done).rise, 0.0);
        e.finish();
        let mid = done + Duration::from_millis(100);
        e.begin(false, mid);
        let t = e.progress(&key("b"), 1, mid).rise;
        assert!(t > 0.0 && t < 1.0, "途中: {t}");
        e.finish();
        // 消えて、また足された。
        e.begin(false, mid);
        e.progress(&key("a"), 0, mid).rise;
        e.finish();
        e.begin(false, mid);
        assert_eq!(e.progress(&key("b"), 1, mid).rise, 0.0);
    }

    /// 隠れていたダッシュボードを出し直すと、出ていたボタンも順に出し直す。
    #[test]
    fn showing_again_replays_the_slide() {
        let (mut e, t0) = (Entering::default(), Instant::now());
        let done = t0 + Duration::from_secs(2);
        e.begin(false, t0);
        e.progress(&key("a"), 0, t0).rise;
        e.finish();
        e.begin(false, done);
        assert_eq!(e.progress(&key("a"), 0, done).rise, 1.0);
        e.finish();
        e.begin(true, done);
        e.progress(&key("a"), 0, done).rise;
        e.finish();
        e.begin(false, done);
        assert_eq!(e.progress(&key("a"), 0, done).rise, 0.0);
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
                e.progress(k, i, now).rise;
            }
            e.finish();
        }
        // 真ん中の b が消えた: 前の場所に、消えていくものとして残る。
        let (cur, prev) = (vec!["a", "c"], vec!["a", "b", "c"]);
        let items = with_leaving(cur, &prev, |k| key(k), &mut e, done);
        assert_eq!(items, [("a", false), ("b", true), ("c", false)]);
        let mid = done + Duration::from_millis(100);
        let out = e.progress_out(&key("b"), mid).rise;
        assert!(out > 0.0, "途中: {out}");
        e.begin(false, mid);
        e.progress(&key("a"), 0, mid).rise;
        e.progress(&key("c"), 2, mid).rise;
        assert!(e.finish(), "消えている間は次のフレームも描く");
        // 終われば、並びから外れる。
        let end = done + Duration::from_secs(2);
        assert_eq!(e.progress_out(&key("b"), end).rise, 0.0);
        e.begin(false, end);
        let items = with_leaving(vec!["a", "c"], &prev, |k| key(k), &mut e, end);
        assert_eq!(items, [("a", false), ("c", false)]);
        assert!(!e.finish());
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
