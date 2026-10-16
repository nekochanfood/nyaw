//! スクロールバー。パネルの中身があふれた軸（縦・横）にだけ出る。
//!
//! - 端が丸い丸棒。ふだんは [`scrollbar::THIN`] の細さで端に寄り、カーソルが触れる（トラックの帯の上・ドラッグ中）と
//!   [`scrollbar::THICK`] まで厚みを取り戻す。
//! - 色は平常 / ホバー / 押下（ドラッグ）で変わる（`track` → `neutral_solid_hover` → `neutral_text_low`）。
//! - 中身が端で引っ張られている間（[`elastic`]）、棒が端で押しつぶされて縮み、戻る（[`Scrollbars::pulled_by`]）。
//! - 棒を掴んで動かす・トラックを押すとその位置へ飛ぶ。
//! - ふだんは見えない。スクロールした・トラックの帯に触れた・掴んだ・引っ張ったときに現れ、止まって [`LINGER`] 経つと消える。
//!
//! 使い方: スクロールする要素（`overflow_*_scroll` + `track_scroll(&handle)`）と同じ大きさの `relative()` な入れ物に、
//! 兄弟として重ねる。
//! ```ignore
//! div().relative().size_full()
//!     .child(div().id("list").size_full().overflow_y_scroll().track_scroll(&handle).children(rows))
//!     .child(scrollbars(&theme, "list-bars", &handle))
//! ```

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Axis, Bounds, DispatchPhase, Div, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, RenderOnce, ScrollDelta, ScrollHandle, ScrollWheelEvent,
    SharedString, Styled, TouchPhase, Window, canvas, div, point, prelude::*, px, rgba,
};

use super::{Fade, keep_drawing};
use crate::{Motion, Theme, duration_ms, radius, scrollbar as spec};

/// 中身がこれだけ（px）引っ張られると、棒の押しつぶしが最大になる。
const SQUASH_RANGE: f32 = 48.0;
/// 引っ張りの重さ。伸びるほど重くなり（対数）、上限はない。`PULL_SOFT` は重くなり始める目安（px）、
/// `PULL_GRIP` は伸び始めの追従（1 = 指と同じだけ動く）。
const PULL_SOFT: f32 = 96.0;
const PULL_GRIP: f32 = 0.6;
/// 離したあとの戻りの時定数（秒）。止まった所からゆっくり動き出し、加速して、長く柔らかく着地する。
const RELEASE_TAU: f32 = 0.11;
/// ホイールが止まって、これだけ経つと離したとみなす。
const HOLD_WHEEL: Duration = Duration::from_millis(120);
/// 終わりの合図が無い指の入力（トラックパッドなど）が止まって、これだけ経つと離したとみなす。
const HOLD_PIXELS: Duration = Duration::from_millis(250);
/// 動きが止まってから棒が消え始めるまで。
const LINGER: Duration = Duration::from_millis(1000);
/// 縦と横が両方出るとき、交わる隅に空ける長さ。
const CORNER: f32 = spec::THICK + 2.0 * spec::MARGIN;
const AXES: [Axis; 2] = [Axis::Vertical, Axis::Horizontal];

/// 棒の位置（軸に沿った px。ビューポートの端から測る）。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Thumb {
    start: f32,
    /// トラックの始まり（端の余白 + 角丸を避ける分）。
    lead: f32,
    len: f32,
    track: f32,
}

/// `view` = ビューポートの長さ、`max` = スクロールできる量、`offset` = 今のオフセット（負 = 進んだ）、
/// `reserve` = 反対の軸の棒に譲る隅、`squash` = 押しつぶし 0..=1、`ends` = 両端で避ける角丸。
/// あふれていなければ `None`。
fn thumb(view: f32, max: f32, offset: f32, reserve: f32, squash: f32, ends: f32) -> Option<Thumb> {
    if max < 1.0 {
        return None;
    }
    let lead = spec::MARGIN + ends;
    let track = (view - 2.0 * lead - reserve).max(0.0);
    let full = (track * view / (view + max)).clamp(spec::MIN_THUMB.min(track), track);
    let len = full * (1.0 - spec::SQUASH * squash.clamp(0.0, 1.0));
    let progress = (-offset / max).clamp(0.0, 1.0);
    Some(Thumb {
        start: lead + progress * (track - len),
        lead,
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
    -((pointer - grab - t.lead) / free).clamp(0.0, 1.0) * max
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
    /// 見え具合 0..=1。
    vis: Smooth,
    /// 直近に見たオフセット。変わったらスクロールしたとみなす。
    seen: f32,
    /// 最後に何か動きがあった時刻。ここから [`LINGER`] の間は見せる。
    woke: Option<Instant>,
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
    /// （マウスのホイールに離す合図は無い。タッチは終わりの合図が来るまで保持する）。
    Held {
        value: f32,
        last: Instant,
        /// `None` は離す合図が来るまで保持する（タッチ）。
        idle: Option<Duration>,
    },
    /// 戻っている（落ち着いていれば `from` = 0）。`from` は離した時点の伸び（px）。
    Release { from: f32, start: Instant },
}

/// 指（ホイール）の動きの積算 `raw` から、中身の伸び（px）を出す。伸びるほど重くなる。
fn stretch(raw: f32) -> f32 {
    raw.signum() * PULL_SOFT * (1.0 + PULL_GRIP * raw.abs() / PULL_SOFT).ln()
}

/// [`stretch`] の逆。
fn unstretch(shown: f32) -> f32 {
    shown.signum() * PULL_SOFT / PULL_GRIP * ((shown.abs() / PULL_SOFT).exp() - 1.0)
}

/// 離したあとの伸び。速度 0 から動き出し（緩）、加速し（急）、尾を引いて着地する（緩）。振動はしない
/// （過減衰の端、臨界減衰: x(t) = x0 (1 + t/τ) e^(-t/τ)）。
fn settle(from: f32, secs: f32) -> f32 {
    let u = secs / RELEASE_TAU;
    let v = from * (1.0 + u) * (-u).exp();
    if v.abs() < 0.5 { 0.0 } else { v }
}

/// `elastic` の状態。
#[derive(Clone, Copy)]
struct ElasticState {
    pull: [Pull; 2],
    /// 指がスライド中（始まりの合図から終わりの合図まで）。この間は手を止めても戻さない。
    touching: bool,
}

type OnWheel = Rc<dyn Fn(&ScrollWheelEvent, &mut Window, &mut App)>;

/// 描画だけを `offset` ずらす。レイアウトは動かないので、スクロールの範囲や位置に影響しない
/// （`top` などで動かすと、末尾側ではスクロール範囲が縮んで引っ張りが打ち消される）。
///
/// ホイールの受け口もここに置く。中身と一緒にずれる要素に付けると、ポインタが端の空いた所へ出たときに届かなくなり、
/// それ以上引っ張れなくなる。ずらす前の位置とスクロール領域の範囲で、ポインタが触れているかを見る。
struct Shift {
    offset: Point<Pixels>,
    child: AnyElement,
    on_wheel: OnWheel,
}

impl IntoElement for Shift {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Shift {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_element_offset(self.offset, |window| child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // 子より先に登録する。ホイールは後に登録したものから受け取るので、入れ子の内側（子）が先、外側のスクロール要素
        // 自身の処理（これより前に登録済み）が後になる。
        let visible = bounds.intersect(&window.content_mask().bounds);
        let on_wheel = self.on_wheel.clone();
        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && visible.contains(&e.position) {
                on_wheel(e, window, cx);
            }
        });
        self.child.paint(window, cx);
    }
}

impl Pull {
    /// いま見えている伸び（px）。
    fn shown(&self, now: Instant) -> f32 {
        match *self {
            Pull::Held { value, .. } => stretch(value),
            Pull::Release { from, start } => {
                settle(from, now.saturating_duration_since(start).as_secs_f32())
            }
        }
    }

    /// 指の動きの積算（戻っている途中でもう一度掴んだら、いまの伸びから続ける）。
    fn raw(&self, now: Instant) -> f32 {
        match *self {
            Pull::Held { value, .. } => value,
            Pull::Release { .. } => unstretch(self.shown(now)),
        }
    }

    fn release(&mut self, now: Instant) {
        if let Pull::Held { value, .. } = *self {
            *self = Pull::Release {
                from: stretch(value),
                start: now,
            };
        }
    }
}

/// 端で引っ張られる中身。スクロール要素の直下の子として、中身を包む。端に着いたままさらにスクロールすると、
/// 中身がどこまでも（伸びるほど重くなりながら）引っ張られ、離す（タッチを離す・ホイールが止まる）と戻る。
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
        let state = window.use_keyed_state(id, cx, |_, _| ElasticState {
            pull: [Pull::Release {
                from: 0.0,
                start: Instant::now(),
            }; 2],
            touching: false,
        });
        let now = Instant::now();
        let mut moving = false;
        // 離したとみなせるものは戻し始め、いまの値を出す。
        let v = state.update(cx, |s, _| {
            s.pull.each_mut().map(|p| {
                if let Pull::Held { last, idle, .. } = *p {
                    moving = true; // 離したかを見張るため描き続ける
                    if idle.is_some_and(|idle| now.saturating_duration_since(last) >= idle) {
                        p.release(now);
                    }
                }
                let shown = p.shown(now);
                moving |= matches!(p, Pull::Release { .. }) && shown != 0.0;
                shown
            })
        });
        cx.default_global::<Pulls>().0.insert(id, v);
        if moving {
            keep_drawing(window, cx);
        }
        // 子はホイールを中身より先に受け取る（スクロールする前の状態で見られる）。
        let on_wheel = Rc::new(
            move |e: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                if !cx.has_global::<Motion>() {
                    return;
                }
                let now = Instant::now();
                let ended = matches!(e.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled);
                let ds = axis_deltas(&handle, e, window);
                let mut used = false;
                state.update(cx, |s, _| {
                    // 指のスライドは始まりと終わりの合図がある。ない入力（ホイール・トラックパッド）は止まったら離したとみなす。
                    match e.touch_phase {
                        TouchPhase::Started => s.touching = true,
                        _ if ended => s.touching = false,
                        _ => {}
                    }
                    let idle = if s.touching {
                        None
                    } else {
                        Some(match e.delta {
                            ScrollDelta::Lines(_) => HOLD_WHEEL,
                            ScrollDelta::Pixels(_) => HOLD_PIXELS,
                        })
                    };
                    for (i, axis) in AXES.into_iter().enumerate() {
                        if ended {
                            // 指を離した（惰性も含む）。戻し始める。引っ張りは惰性で増やさない。
                            s.pull[i].release(now);
                            continue;
                        }
                        let (d, cur) = (ds[i], s.pull[i].raw(now));
                        let reeling = cur != 0.0 && d * cur < 0.0;
                        if d != 0.0 && (reeling || at_edge(&handle, axis, d)) {
                            let mut value = cur + d;
                            if value * cur < 0.0 {
                                value = 0.0; // 戻しきった
                            }
                            s.pull[i] = Pull::Held {
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
        );
        Shift {
            offset: point(px(v[1]), px(v[0])),
            child: base.into_any_element(),
            on_wheel,
        }
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
        if (dx && sx) || (dy && (sx || sy)) {
            cx.stop_propagation();
        }
    })
}

/// 重ねるスクロールバー。[`scrollbars`] で作る。
#[derive(IntoElement)]
pub struct Scrollbars {
    id: SharedString,
    handle: ScrollHandle,
    pull: Option<&'static str>,
    ends: f32,
    always: bool,
    rest: u32,
    hover: u32,
    active: u32,
}

/// `handle` がスクロールする要素のスクロールバー（縦・横とも）。`id` は部品ごとに一意に。
/// 縦は `{id}-v`、横は `{id}-h`（棒は `-thumb` 付き）で指せる。
pub fn scrollbars(theme: &Theme, id: impl Into<SharedString>, handle: &ScrollHandle) -> Scrollbars {
    Scrollbars {
        id: id.into(),
        handle: handle.clone(),
        pull: None,
        ends: 0.0,
        always: false,
        rest: theme.track,
        hover: theme.neutral_text_low,
        active: theme.neutral_text,
    }
}

impl Scrollbars {
    /// 中身を包む [`elastic`] の id。中身が端で引っ張られている間、棒が押しつぶされる。
    pub fn pulled_by(mut self, elastic: &'static str) -> Self {
        self.pull = Some(elastic);
        self
    }

    /// 入れ物の角丸。棒のトラックを両端でこの長さだけ縮め、角の丸みに棒がかからないようにする。
    /// パネルの縁に沿って付けるときは、パネルの角丸（`radius::XL`）を渡す。
    pub fn corner_radius(mut self, r: f32) -> Self {
        self.ends = r;
        self
    }

    /// 動きが無くても常に見せる（既定では、動きが止まると消える）。
    pub fn always_visible(mut self) -> Self {
        self.always = true;
        self
    }
}

impl RenderOnce for Scrollbars {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Scrollbars {
            id,
            handle,
            pull,
            ends,
            always,
            rest,
            hover,
            active,
        } = self;
        let state = window.use_keyed_state(id.clone(), cx, move |_, _| State {
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
                vis: Smooth::rest(if always { 1.0 } else { 0.0 }),
                seen: 0.0,
                woke: None,
            }; 2],
            shown: [false; 2],
        });
        let now = Instant::now();
        let animated = cx.has_global::<Motion>();
        let reads = AXES.map(|a| read(&handle, a));
        let pulled = pull
            .and_then(|p| cx.try_global::<Pulls>()?.0.get(p).copied())
            .unwrap_or_default();
        let st = state.update(cx, |s, _| {
            for (i, b) in s.bars.iter_mut().enumerate() {
                let off = reads[i].2;
                if off != b.seen {
                    b.seen = off;
                    b.woke = Some(now);
                }
                if b.zone || b.slide || b.grab.is_some() || pulled[i] != 0.0 {
                    b.woke = Some(now);
                }
                let awake = always
                    || b.woke.is_some_and(|w| now.saturating_duration_since(w) < LINGER);
                b.vis.go(if awake { 1.0 } else { 0.0 }, now, animated);
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

        let mut moving = false;
        let mut overlay = div().absolute().inset_0();
        for (i, axis) in AXES.into_iter().enumerate() {
            let (view, max, off, reserve) = reads[i];
            let bar = st.bars[i];
            let squash = (pulled[i].abs() / SQUASH_RANGE).min(1.0);
            let (thick, m2) = bar.thick.at(now);
            let (color, m3) = bar.color.at(now);
            let (vis, m4) = bar.vis.at(now);
            // 消えるまでの時間切れに気づけるよう、見せている間も描き続ける。
            let lingering = bar.woke.is_some_and(|w| now.saturating_duration_since(w) < LINGER);
            moving |= m2 | m3 | m4 | lingering;
            let Some(t) = thumb(view, max, off, reserve, squash, ends) else {
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
                    let Some(t) = thumb(view, max, off, reserve, 0.0, ends) else {
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
                            let Some(t) = thumb(view, max, off, reserve, 0.0, ends) else {
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
                .opacity(vis)
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
                            if let Some(t) = thumb(view, max, off, reserve, 0.0, ends) {
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
        let t = thumb(100.0, 100.0, 0.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(t.track, 100.0 - 2.0 * spec::MARGIN);
        assert!((t.len - t.track / 2.0).abs() < 1e-3);
        assert_eq!(t.start, spec::MARGIN);
        // 一番下まで進むと棒の末尾がトラックの末尾に着く。
        let end = thumb(100.0, 100.0, -100.0, 0.0, 0.0, 0.0).unwrap();
        assert!((end.start + end.len - (spec::MARGIN + end.track)).abs() < 1e-3);
        // あふれていなければ出ない。
        assert_eq!(thumb(100.0, 0.0, 0.0, 0.0, 0.0, 0.0), None);
    }

    #[test]
    fn corner_radius_keeps_the_track_off_both_rounded_ends() {
        let r = radius::XL;
        let t = thumb(200.0, 100.0, 0.0, 0.0, 0.0, r).unwrap();
        assert_eq!(t.start, spec::MARGIN + r);
        assert_eq!(t.track, 200.0 - 2.0 * (spec::MARGIN + r));
        // 一番下まで進んでも、末尾は下の角丸の手前で止まる。
        let end = thumb(200.0, 100.0, -100.0, 0.0, 0.0, r).unwrap();
        assert!((end.start + end.len - (200.0 - spec::MARGIN - r)).abs() < 1e-3);
        // 掴んで動かしても、縮めたトラックの上でオフセットに戻る。
        assert!((offset_for(&end, end.start + 1.0, 1.0, 100.0) + 100.0).abs() < 1e-2);
    }

    #[test]
    fn thumb_has_a_minimum_and_squashes_at_the_end() {
        let tiny = thumb(100.0, 100000.0, 0.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(tiny.len, spec::MIN_THUMB);
        let squashed = thumb(100.0, 100.0, -100.0, 0.0, 1.0, 0.0).unwrap();
        let plain = thumb(100.0, 100.0, -100.0, 0.0, 0.0, 0.0).unwrap();
        assert!(squashed.len < plain.len);
        // 押しつぶされても末尾（押し込んでいる端）は動かない。
        assert!((squashed.start + squashed.len - (plain.start + plain.len)).abs() < 1e-3);
    }

    #[test]
    fn dragging_maps_pointer_back_to_offset() {
        let max = 300.0;
        let t = thumb(100.0, max, -150.0, 0.0, 0.0, 0.0).unwrap();
        // 今の位置を掴んだまま動かさなければオフセットは変わらない。
        let grab = 5.0;
        let got = offset_for(&t, t.start + grab, grab, max);
        assert!((got + 150.0).abs() < 1e-2);
        // 行き過ぎても範囲に収まる。
        assert_eq!(offset_for(&t, -1000.0, grab, max), 0.0);
        assert_eq!(offset_for(&t, 1000.0, grab, max), -max);
    }

    #[test]
    fn pull_gets_heavier_without_a_limit_and_inverts() {
        let (a, b, c) = (stretch(100.0), stretch(200.0), stretch(10000.0));
        assert!(
            a > 0.0 && b - a < a,
            "伸びるほど同じ入力での伸びが小さくなる"
        );
        assert!(c > b, "上限はない");
        assert_eq!(stretch(-100.0), -a);
        assert!((unstretch(stretch(250.0)) - 250.0).abs() < 1e-2);
    }

    #[test]
    fn release_eases_in_then_out_without_overshoot() {
        let at = |ms: f32| settle(100.0, ms / 1000.0);
        let (early, middle) = (100.0 - at(50.0), at(50.0) - at(150.0));
        assert!(
            early < middle,
            "動き出しはゆっくり、そのあと加速する: {early} {middle}"
        );
        assert!(
            at(150.0) - at(250.0) > at(450.0) - at(550.0),
            "終わりへ向けて緩やかになる"
        );
        let samples: Vec<f32> = (0..100).map(|i| at(i as f32 * 10.0)).collect();
        assert!(samples.windows(2).all(|w| w[1] <= w[0]), "行き過ぎない");
        assert_eq!(at(2000.0), 0.0, "最後は 0 に落ち着く");
    }
}
