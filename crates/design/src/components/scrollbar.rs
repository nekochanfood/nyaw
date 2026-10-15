//! スクロールバー。パネルの中身があふれた軸（縦・横）にだけ出る。
//!
//! - 端が丸い丸棒。ふだんは [`scrollbar::THIN`] の細さで端に寄り、カーソルが触れる（トラックの帯の上・ドラッグ中）と
//!   [`scrollbar::THICK`] まで厚みを取り戻す。
//! - 色は平常 / ホバー / 押下（ドラッグ）で変わる（`track` → `neutral_solid_hover` → `neutral_text_low`）。
//! - 中身が端で引っ張られている間（[`elastic`]）、棒が端で押しつぶされて縮み、戻る（[`Scrollbars::pulled_by`]）。
//! - 棒を掴んで動かす・トラックを押すとその位置へ飛ぶ。
//!
//! 使い方: スクロールする要素（`overflow_*_scroll` + `track_scroll(&handle)`）と同じ大きさの `relative()` な入れ物に、
//! 兄弟として重ねる。
//! ```ignore
//! div().relative().size_full()
//!     .child(div().id("list").size_full().overflow_y_scroll().track_scroll(&handle).children(rows))
//!     .child(scrollbars(&theme, "list-bars", &handle))
//! ```

use std::time::{Duration, Instant};

use gpui::{
    App, Axis, Div, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, RenderOnce, ScrollDelta, ScrollHandle, ScrollWheelEvent, SharedString, Styled,
    TouchPhase, Window, canvas, div, prelude::*, px, rgba,
};

use super::{Fade, keep_drawing};
use crate::{Motion, Theme, duration_ms, radius, scrollbar as spec};

/// 中身がこれだけ（px）引っ張られると、棒の押しつぶしが最大になる。
const SQUASH_RANGE: f32 = 48.0;
/// 端でのスクロール量に対する、中身の引っ張られ方（1 より小さいほど重い）。
const PULL_RESIST: f32 = 0.5;
/// ホイールが止まって、これだけ経つと離したとみなす。
const HOLD_WHEEL: Duration = Duration::from_millis(120);
/// 指のスライド（またはトラックパッド）に終わりの合図が来なかったとき、離したとみなすまで。
const HOLD_TOUCH: Duration = Duration::from_millis(400);
/// 縦と横が両方出るとき、交わる隅に空ける長さ。
const CORNER: f32 = spec::THICK + 2.0 * spec::MARGIN;
const AXES: [Axis; 2] = [Axis::Vertical, Axis::Horizontal];

/// 棒の位置（軸に沿った px。ビューポートの端から測る）。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Thumb {
    start: f32,
    len: f32,
    track: f32,
}

/// `view` = ビューポートの長さ、`max` = スクロールできる量、`offset` = 今のオフセット（負 = 進んだ）、
/// `reserve` = 反対の軸の棒に譲る隅、`squash` = 押しつぶし 0..=1。あふれていなければ `None`。
fn thumb(view: f32, max: f32, offset: f32, reserve: f32, squash: f32) -> Option<Thumb> {
    if max < 1.0 {
        return None;
    }
    let track = (view - 2.0 * spec::MARGIN - reserve).max(0.0);
    let full = (track * view / (view + max)).clamp(spec::MIN_THUMB.min(track), track);
    let len = full * (1.0 - spec::SQUASH * squash.clamp(0.0, 1.0));
    let progress = (-offset / max).clamp(0.0, 1.0);
    Some(Thumb {
        start: spec::MARGIN + progress * (track - len),
        len,
        track,
    })
}

/// 棒の先頭が `pointer - grab` に来るときのオフセット。
fn offset_for(t: &Thumb, pointer: f32, grab: f32, max: f32) -> f32 {
    let free = t.track - t.len;
    if free <= 0.0 {
        return 0.0;
    }
    -((pointer - grab - spec::MARGIN) / free).clamp(0.0, 1.0) * max
}

/// 値が `NORMAL` かけて ease-out で動く。
#[derive(Clone, Copy)]
struct Smooth {
    from: f32,
    to: f32,
    start: Option<Instant>,
}

impl Smooth {
    const fn rest(v: f32) -> Self {
        Smooth {
            from: v,
            to: v,
            start: None,
        }
    }

    fn at(&self, now: Instant) -> (f32, bool) {
        let Some(start) = self.start else {
            return (self.to, false);
        };
        let t = now.saturating_duration_since(start).as_secs_f32() * 1000.0
            / duration_ms::NORMAL as f32;
        if t >= 1.0 {
            return (self.to, false);
        }
        let e = 1.0 - (1.0 - t).powi(3);
        (self.from + (self.to - self.from) * e, true)
    }

    /// 今の値から `to` へ動き直す（`animated` でなければ即座）。
    fn go(&mut self, to: f32, now: Instant, animated: bool) {
        if self.to != to {
            let (cur, _) = self.at(now);
            *self = if animated {
                Smooth {
                    from: cur,
                    to,
                    start: Some(now),
                }
            } else {
                Smooth::rest(to)
            };
        }
    }
}

#[derive(Clone, Copy)]
struct Bar {
    /// トラックの帯にカーソルがある。
    zone: bool,
    /// 棒にカーソルがある。
    over: bool,
    /// 掴んでいる（棒の先頭からカーソルまでの px）。
    grab: Option<f32>,
    /// 指のスライド（タッチのスワイプ）で動かしている。掴むのに長押しは要らない。
    slide: bool,
    thick: Smooth,
    color: Fade,
}

#[derive(Clone, Copy)]
struct State {
    bars: [Bar; 2],
    /// 直近の描画で出していた軸。レイアウト後に変わったら描き直す。
    shown: [bool; 2],
}

/// (ビューポートの長さ, あふれる量, オフセット, 反対の軸に譲る隅)
fn read(handle: &ScrollHandle, axis: Axis) -> (f32, f32, f32, f32) {
    let (b, o, m) = (handle.bounds(), handle.offset(), handle.max_offset());
    let corner = |other: Pixels| if f32::from(other) >= 1.0 { CORNER } else { 0.0 };
    match axis {
        Axis::Vertical => (
            f32::from(b.size.height),
            f32::from(m.y),
            f32::from(o.y),
            corner(m.x),
        ),
        Axis::Horizontal => (
            f32::from(b.size.width),
            f32::from(m.x),
            f32::from(o.x),
            corner(m.y),
        ),
    }
}

/// ウィンドウ上の点を、ビューポートの端から軸に沿って測った px にする。
fn along(handle: &ScrollHandle, axis: Axis, p: Point<Pixels>) -> f32 {
    let o = handle.bounds().origin;
    match axis {
        Axis::Vertical => f32::from(p.y - o.y),
        Axis::Horizontal => f32::from(p.x - o.x),
    }
}

fn set_offset(handle: &ScrollHandle, axis: Axis, v: f32) {
    let mut o = handle.offset();
    match axis {
        Axis::Vertical => o.y = px(v),
        Axis::Horizontal => o.x = px(v),
    }
    handle.set_offset(o);
}

/// 斜めにも滑らかに動くスクロール領域にする。gpui は既定で、縦横が同時に動くとき大きい側の軸だけを使うので、
/// 縦横ともあふれる領域で斜めや円を描くとカクつく。縦横ともスクロールする要素にはこれを通す。
pub fn free_scroll<E: Styled>(mut element: E) -> E {
    element.style().allow_concurrent_scroll = Some(true);
    element
}

/// ホイール（や指のスライド）の、軸ごとの動き量（gpui と同じ割り当て: 横だけあふれているときは縦のホイールも横を動かす）。
fn axis_deltas(handle: &ScrollHandle, e: &ScrollWheelEvent, window: &Window) -> [f32; 2] {
    let d = e.delta.pixel_delta(window.line_height());
    let (dx, dy) = (f32::from(d.x), f32::from(d.y));
    let only_x = f32::from(handle.max_offset().y) < 1.0;
    [
        dy,
        if dx != 0.0 {
            dx
        } else if only_x {
            dy
        } else {
            0.0
        },
    ]
}

/// 先頭か末尾に着いたまま、さらに外へ押しているか（`d` > 0 = 先頭を越えて引く向き）。
/// 内側の要素がホイールを受け取る前の状態で見る必要がある。
fn at_edge(handle: &ScrollHandle, axis: Axis, d: f32) -> bool {
    let (_, max, off, _) = read(handle, axis);
    max >= 1.0 && ((off >= -0.5 && d > 0.0) || (off <= -max + 0.5 && d < 0.0))
}

/// いまの中身の引っ張り量（軸ごとの px。`elastic` の id ごと）。棒の押しつぶしがこれを見る。
#[derive(Default)]
struct Pulls(std::collections::HashMap<&'static str, [f32; 2]>);

impl gpui::Global for Pulls {}

/// 引っ張りの状態。触れている間は動かさず、離したら戻る。
#[derive(Clone, Copy)]
enum Pull {
    /// 引っ張っている。`idle` の間、新しい入力が無ければ離したとみなす
    /// （マウスのホイールに離す合図は無く、タッチは終わりの合図が来る）。
    Held {
        value: f32,
        last: Instant,
        idle: Duration,
    },
    /// 戻っている（落ち着いていれば 0）。
    Release(Smooth),
}

impl Pull {
    fn value(&self, now: Instant) -> f32 {
        match self {
            Pull::Held { value, .. } => *value,
            Pull::Release(s) => s.at(now).0,
        }
    }

    fn release(&mut self, now: Instant) {
        if let Pull::Held { value, .. } = *self {
            *self = Pull::Release(Smooth {
                from: value,
                to: 0.0,
                start: Some(now),
            });
        }
    }
}

/// 端で引っ張られる中身。スクロール要素の直下の子として、中身を包む。端に着いたままさらにスクロールすると、
/// 中身がどこまでも（重みを付けて）引っ張られ、離す（タッチを離す・ホイールが止まる）と戻る。
/// 反対向きにスクロールすれば、引っ張りが先に戻ってからスクロールする。
/// 中身は少なくともビューポートの大きさになる（どこをポインタが指していても受け取れるように）。
#[derive(IntoElement)]
pub struct Elastic {
    id: &'static str,
    handle: ScrollHandle,
    base: Div,
}

pub fn elastic(id: &'static str, handle: &ScrollHandle) -> Elastic {
    Elastic {
        id,
        handle: handle.clone(),
        base: div().relative().min_w_full().min_h_full(),
    }
}

impl Styled for Elastic {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Elastic {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.base.extend(elements)
    }
}

impl RenderOnce for Elastic {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Elastic { id, handle, base } = self;
        let state = window.use_keyed_state(id, cx, |_, _| [Pull::Release(Smooth::rest(0.0)); 2]);
        let now = Instant::now();
        let mut moving = false;
        // 離したとみなせるものは戻し始め、いまの値を出す。
        let v = state.update(cx, |s, _| {
            s.each_mut().map(|p| {
                if let Pull::Held { last, idle, .. } = *p {
                    moving = true; // 離したかを見張るため描き続ける
                    if now.saturating_duration_since(last) >= idle {
                        p.release(now);
                    }
                }
                if let Pull::Release(r) = p {
                    moving |= r.at(now).1;
                }
                p.value(now)
            })
        });
        cx.default_global::<Pulls>().0.insert(id, v);
        if moving {
            keep_drawing(window, cx);
        }
        // 子はホイールを中身より先に受け取る（スクロールする前の状態で見られる）。
        base.top(px(v[0])).left(px(v[1])).on_scroll_wheel(
            move |e: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                if !cx.has_global::<Motion>() {
                    return;
                }
                let now = Instant::now();
                let ended = matches!(e.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled);
                let idle = match e.delta {
                    ScrollDelta::Lines(_) => HOLD_WHEEL,
                    ScrollDelta::Pixels(_) => HOLD_TOUCH,
                };
                let ds = axis_deltas(&handle, e, window);
                let mut used = false;
                state.update(cx, |s, _| {
                    for (i, axis) in AXES.into_iter().enumerate() {
                        if ended {
                            // 指を離した（惰性も含む）。戻し始める。引っ張りは惰性で増やさない。
                            s[i].release(now);
                            continue;
                        }
                        let (d, cur) = (ds[i], s[i].value(now));
                        let reeling = cur != 0.0 && d * cur < 0.0;
                        if d != 0.0 && (reeling || at_edge(&handle, axis, d)) {
                            let mut value = cur + d * PULL_RESIST;
                            if value * cur < 0.0 {
                                value = 0.0; // 戻しきった
                            }
                            s[i] = Pull::Held {
                                value,
                                last: now,
                                idle,
                            };
                            used = true;
                        }
                    }
                });
                if used {
                    // 引っ張っている間は中身をスクロールさせず、外側にも流さない。
                    cx.stop_propagation();
                    window.refresh();
                }
            },
        )
    }
}

/// 入れ子のスクロール要素のうち、内側の要素にこれを通す。gpui はポインタの下にあるスクロール要素すべてに同じホイールを配るので、
/// 通さないと内側と外側が同時に動いて操作しづらい。通すと、ポインタが触れている最も内側のあふれる要素だけが受け取る
/// （内側が端に着いていても外側へは流さない）。`handle` はその要素の `track_scroll` と同じもの。
pub fn claim_scroll<E: InteractiveElement>(element: E, handle: &ScrollHandle) -> E {
    let handle = handle.clone();
    element.on_scroll_wheel(move |e, window, cx| {
        let d = e.delta.pixel_delta(window.line_height());
        let (dx, dy) = (f32::from(d.x) != 0.0, f32::from(d.y) != 0.0);
        let m = handle.max_offset();
        let (sx, sy) = (f32::from(m.x) >= 1.0, f32::from(m.y) >= 1.0);
        // 横だけあふれる要素は縦のホイールも横に受ける（gpui と同じ）。
        if (dx && sx) || (dy && sy) || (dy && sx && !sy) {
            cx.stop_propagation();
        }
    })
}

/// 重ねるスクロールバー。[`scrollbars`] で作る。
#[derive(IntoElement)]
pub struct Scrollbars {
    id: &'static str,
    handle: ScrollHandle,
    pull: Option<&'static str>,
    rest: u32,
    hover: u32,
    active: u32,
}

/// `handle` がスクロールする要素のスクロールバー（縦・横とも）。`id` は部品ごとに一意に。
/// 縦は `{id}-v`、横は `{id}-h`（棒は `-thumb` 付き）で指せる。
pub fn scrollbars(theme: &Theme, id: &'static str, handle: &ScrollHandle) -> Scrollbars {
    Scrollbars {
        id,
        handle: handle.clone(),
        pull: None,
        rest: theme.track,
        hover: theme.neutral_solid_hover,
        active: theme.neutral_text_low,
    }
}

impl Scrollbars {
    /// 中身を包む [`elastic`] の id。中身が端で引っ張られている間、棒が押しつぶされる。
    pub fn pulled_by(mut self, elastic: &'static str) -> Self {
        self.pull = Some(elastic);
        self
    }
}

impl RenderOnce for Scrollbars {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Scrollbars {
            id,
            handle,
            pull,
            rest,
            hover,
            active,
        } = self;
        let state = window.use_keyed_state(id, cx, move |_, _| State {
            bars: [Bar {
                zone: false,
                over: false,
                grab: None,
                slide: false,
                thick: Smooth::rest(0.0),
                color: Fade {
                    from: rest,
                    to: rest,
                    start: None,
                },
            }; 2],
            shown: [false; 2],
        });
        let now = Instant::now();
        let animated = cx.has_global::<Motion>();
        let reads = AXES.map(|a| read(&handle, a));
        let st = state.update(cx, |s, _| {
            for (i, b) in s.bars.iter_mut().enumerate() {
                b.thick.go(
                    if b.zone || b.slide || b.grab.is_some() {
                        1.0
                    } else {
                        0.0
                    },
                    now,
                    animated,
                );
                let to = if b.grab.is_some() || b.slide {
                    active
                } else if b.over {
                    hover
                } else {
                    rest
                };
                if b.color.to != to {
                    let (from, _) = b.color.at(now);
                    b.color = if animated {
                        Fade {
                            from,
                            to,
                            start: Some(now),
                        }
                    } else {
                        Fade {
                            from: to,
                            to,
                            start: None,
                        }
                    };
                }
                s.shown[i] = reads[i].1 >= 1.0;
            }
            *s
        });

        let pulled = pull
            .and_then(|p| cx.try_global::<Pulls>()?.0.get(p).copied())
            .unwrap_or_default();
        let mut moving = false;
        let mut overlay = div().absolute().inset_0();
        for (i, axis) in AXES.into_iter().enumerate() {
            let (view, max, off, reserve) = reads[i];
            let bar = st.bars[i];
            let squash = (pulled[i].abs() / SQUASH_RANGE).min(1.0);
            let (thick, m2) = bar.thick.at(now);
            let (color, m3) = bar.color.at(now);
            moving |= m2 | m3;
            let Some(t) = thumb(view, max, off, reserve, squash) else {
                continue;
            };
            let v = axis == Axis::Vertical;
            let name = format!("{id}-{}", if v { "v" } else { "h" });
            let girth = spec::THIN + (spec::THICK - spec::THIN) * thick;
            let band = spec::THICK + 2.0 * spec::MARGIN;

            let zone = {
                let state = state.clone();
                move |on: &bool, window: &mut Window, cx: &mut App| {
                    state.update(cx, |s, _| s.bars[i].zone = *on);
                    window.refresh();
                }
            };
            let zone_leave = state.clone();
            let over = {
                let state = state.clone();
                move |on: &bool, window: &mut Window, cx: &mut App| {
                    state.update(cx, |s, _| s.bars[i].over = *on);
                    window.refresh();
                }
            };
            let press = {
                let (state, handle) = (state.clone(), handle.clone());
                move |e: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                    let (view, max, off, reserve) = read(&handle, axis);
                    let Some(t) = thumb(view, max, off, reserve, 0.0) else {
                        return;
                    };
                    let p = along(&handle, axis, e.position);
                    // 棒を掴んだらそのまま、トラックを押したら棒の中心がカーソルに来るよう飛ぶ。
                    let grab = if (t.start..=t.start + t.len).contains(&p) {
                        p - t.start
                    } else {
                        t.len / 2.0
                    };
                    set_offset(&handle, axis, offset_for(&t, p, grab, max));
                    state.update(cx, |s, _| s.bars[i].grab = Some(grab));
                    window.refresh();
                    cx.stop_propagation();
                }
            };

            // 指のスライド: 押してそのまま動かすとタッチはスクロールとして届く。棒の上で始まったものは棒を動かす
            // （長押しは要らない）。届く位置は押した所に固定されるので、カーソルが外へ出ても続く。
            let slide = {
                let (state, handle) = (state.clone(), handle.clone());
                move |e: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                    let ScrollDelta::Pixels(d) = e.delta else {
                        return; // ホイールの目盛りは中身をスクロールする。
                    };
                    let d = f32::from(if v { d.y } else { d.x });
                    match e.touch_phase {
                        TouchPhase::Started | TouchPhase::Moved => {
                            let (view, max, off, reserve) = read(&handle, axis);
                            let Some(t) = thumb(view, max, off, reserve, 0.0) else {
                                return;
                            };
                            let free = t.track - t.len;
                            if d == 0.0 || free <= 0.0 {
                                return;
                            }
                            // 指が d 動くと棒も d 動く。中身はその max / free 倍、逆向きに進む。
                            set_offset(&handle, axis, (off - d * max / free).clamp(-max, 0.0));
                            state.update(cx, |s, _| s.bars[i].slide = true);
                        }
                        // 終わり（惰性も含む）。中身を惰性で流さない。
                        _ => state.update(cx, |s, _| s.bars[i].slide = false),
                    }
                    window.refresh();
                    cx.stop_propagation();
                }
            };
            let strip = div()
                .id(SharedString::from(name.clone()))
                .debug_selector({
                    let n = name.clone();
                    move || n.clone()
                })
                .absolute()
                .block_mouse_except_scroll()
                .on_hover(move |on, w, cx| {
                    if !*on {
                        // カーソルが離れたら、終わりの知らせが来なかったスライドも解く。
                        zone_leave.update(cx, |s, _| s.bars[i].slide = false);
                    }
                    zone(on, w, cx)
                })
                .on_scroll_wheel(slide)
                .on_mouse_down(MouseButton::Left, press);
            let strip = if v {
                strip.top_0().right_0().bottom(px(reserve)).w(px(band))
            } else {
                strip.left_0().bottom_0().right(px(reserve)).h(px(band))
            };
            let knob = div()
                .id(SharedString::from(format!("{name}-thumb")))
                .debug_selector({
                    let n = format!("{name}-thumb");
                    move || n.clone()
                })
                .absolute()
                .rounded(px(radius::FULL))
                .bg(rgba(color))
                .on_hover(move |on, w, cx| over(on, w, cx));
            let knob = if v {
                knob.top(px(t.start))
                    .right(px(spec::MARGIN))
                    .w(px(girth))
                    .h(px(t.len))
            } else {
                knob.left(px(t.start))
                    .bottom(px(spec::MARGIN))
                    .h(px(girth))
                    .w(px(t.len))
            };
            overlay = overlay.child(strip.child(knob));
        }
        if moving {
            keep_drawing(window, cx);
        }

        // 描画後の仕事: レイアウトで出る軸が変わったら描き直す。掴んでいる間はウィンドウのどこでもカーソルに追従する。
        let watch = {
            let (state, handle) = (state.clone(), handle.clone());
            canvas(
                |_, _, _| (),
                move |_, _, window, cx| {
                    let now_shown = AXES.map(|a| read(&handle, a).1 >= 1.0);
                    if state.read(cx).shown != now_shown {
                        keep_drawing(window, cx);
                    }
                    let (s, h) = (state.clone(), handle.clone());
                    window.on_mouse_event(move |e: &MouseMoveEvent, _, window, cx| {
                        for (i, axis) in AXES.into_iter().enumerate() {
                            let Some(grab) = s.read(cx).bars[i].grab else {
                                continue;
                            };
                            if e.pressed_button != Some(MouseButton::Left) {
                                s.update(cx, |s, _| s.bars[i].grab = None);
                                window.refresh();
                                continue;
                            }
                            let (view, max, off, reserve) = read(&h, axis);
                            if let Some(t) = thumb(view, max, off, reserve, 0.0) {
                                let p = along(&h, axis, e.position);
                                set_offset(&h, axis, offset_for(&t, p, grab, max));
                                window.refresh();
                            }
                        }
                    });
                    let s = state.clone();
                    window.on_mouse_event(move |e: &MouseUpEvent, _, window, cx| {
                        if e.button == MouseButton::Left {
                            s.update(cx, |s, _| s.bars.iter_mut().for_each(|b| b.grab = None));
                            window.refresh();
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        };

        overlay.child(watch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_spans_the_track_in_proportion() {
        // ビューポート 100、中身 200（あふれ 100）→ 棒は半分。
        let t = thumb(100.0, 100.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(t.track, 100.0 - 2.0 * spec::MARGIN);
        assert!((t.len - t.track / 2.0).abs() < 1e-3);
        assert_eq!(t.start, spec::MARGIN);
        // 一番下まで進むと棒の末尾がトラックの末尾に着く。
        let end = thumb(100.0, 100.0, -100.0, 0.0, 0.0).unwrap();
        assert!((end.start + end.len - (spec::MARGIN + end.track)).abs() < 1e-3);
        // あふれていなければ出ない。
        assert_eq!(thumb(100.0, 0.0, 0.0, 0.0, 0.0), None);
    }

    #[test]
    fn thumb_has_a_minimum_and_squashes_at_the_end() {
        let tiny = thumb(100.0, 100000.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(tiny.len, spec::MIN_THUMB);
        let squashed = thumb(100.0, 100.0, -100.0, 0.0, 1.0).unwrap();
        let plain = thumb(100.0, 100.0, -100.0, 0.0, 0.0).unwrap();
        assert!(squashed.len < plain.len);
        // 押しつぶされても末尾（押し込んでいる端）は動かない。
        assert!((squashed.start + squashed.len - (plain.start + plain.len)).abs() < 1e-3);
    }

    #[test]
    fn dragging_maps_pointer_back_to_offset() {
        let max = 300.0;
        let t = thumb(100.0, max, -150.0, 0.0, 0.0).unwrap();
        // 今の位置を掴んだまま動かさなければオフセットは変わらない。
        let grab = 5.0;
        let got = offset_for(&t, t.start + grab, grab, max);
        assert!((got + 150.0).abs() < 1e-2);
        // 行き過ぎても範囲に収まる。
        assert_eq!(offset_for(&t, -1000.0, grab, max), 0.0);
        assert_eq!(offset_for(&t, 1000.0, grab, max), -max);
    }
}
