//! カルーセル: 横に並ぶ項目を左右に滑らせて 1 つ選ぶ（端はつながって回る）。並べ方（[`Shape`]）は問わない（弧は [`arc`]）。
//! 押したまま動かす・離すと近い項目に収まる・中央でない項目を押す、の操作と、ここで始まった横の動きを外へ渡さないことを持つ。
//! 責務の分け方は `crates/design/AGENT.md` の「カルーセル」。

mod arc;
pub use arc::arc;

use std::{rc::Rc, time::Instant};

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Radians, RenderOnce, ScrollDelta,
    ScrollWheelEvent, TouchPhase, Window, canvas, div, prelude::*, px, relative, rgba,
};

use super::keep_drawing;
use crate::Motion;
use crate::animation::{Motion as Kind, Transition};

/// 動かすと決めるまでのマウスの動き（px）。タッチは UI 層が同じだけ待ってから送ってくる。
const SLOP: f32 = 12.0;
/// 離す直前の 1 イベントの動き（px）がこれ以上なら、近い項目ではなく払った向きの先の項目に収まる。
const FLING: f32 = 8.0;

/// 項目 1 つの置き場所（px。カルーセルの左右中央・上端が原点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    /// 中心の位置（右・下が正）。
    pub x: f32,
    pub y: f32,
    /// 一辺の長さ。
    pub side: f32,
    /// 不透明度。項目は円として、上に地の色を `1 - alpha` で重ねて薄くする（下の [`carousel`]）。
    pub alpha: f32,
    /// 傾き（ラジアン。時計回りが正）。項目の中心の周りに回して描く（押せる範囲は回さない）。
    pub angle: f32,
}

/// カルーセルの並べ方。中央から `d` 個ずれた項目（小数は動いている途中）をどこに置くかを決める。
#[derive(Clone)]
pub struct Shape {
    /// 項目 1 つ分の指の動き（px）。
    pub step: f32,
    /// カルーセルの高さ（px）。
    pub height: f32,
    /// これ以上ずれた項目は描かない（項目の数）。
    pub reach: f32,
    place: Rc<dyn Fn(f32) -> Place>,
}

impl Shape {
    pub fn new(step: f32, height: f32, reach: f32, place: impl Fn(f32) -> Place + 'static) -> Self {
        Self {
            step,
            height,
            reach,
            place: Rc::new(place),
        }
    }

    /// 中央から `d` 個ずれた項目の置き場所。
    pub fn place(&self, d: f32) -> Place {
        (self.place)(d)
    }
}

/// 位置 `pos` から見た `i` 番目の項目のずれ（項目の数。-len/2 以上 len/2 未満で、端はつながる）。
fn offset(i: usize, pos: f32, len: usize) -> f32 {
    let n = len as f32;
    (i as f32 - pos + n / 2.0).rem_euclid(n) - n / 2.0
}

/// 離したときに収まる位置: 近い項目。払っていれば（直近の動き `velocity` が [`FLING`] 以上。右が正）その向きの先の項目。
fn settle(pos: f32, velocity: f32) -> f32 {
    if velocity >= FLING {
        pos.floor()
    } else if velocity <= -FLING {
        pos.ceil()
    } else {
        pos.round()
    }
}

/// 押してから離すまでのジェスチャ。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    Idle,
    /// マウスで押した（まだ動かすと決まっていない）。押した位置。
    Pressed(Point<Pixels>),
    /// 動かしている。始めたときの位置、指の動きの合計（px。右が正）、直近の動き（px / イベント）。
    /// `start` はマウスのとき押した位置の x（タッチは `None`。動き量を足していく）。
    Dragging {
        from: f32,
        moved: f32,
        velocity: f32,
        start: Option<f32>,
    },
    /// タッチで動かして離した後。UI 層の慣性のスクロールが続くので、次の指のスライドまで飲み込む。
    Coasting,
}

struct State {
    /// 中央にある項目の番号（回った数を含む通し番号。小数は途中）。
    pos: Transition,
    /// 収まる先の項目と、項目の数（前の描画のもの）。
    selected: usize,
    len: usize,
    gesture: Gesture,
    /// 直近の描画での範囲。
    bounds: Bounds<Pixels>,
}

impl State {
    /// 今の位置と、動いているか。`step` は項目 1 つ分の指の動き（px）。
    fn pos(&self, now: Instant, step: f32) -> (f32, bool) {
        match self.gesture {
            Gesture::Dragging { from, moved, .. } => (from - moved / step, false),
            _ => self.pos.at(now),
        }
    }

    /// 指を離した。収まる項目へ動き出し、その番号を返す（動かしていなければ `None`）。
    fn release(&mut self, step: f32, now: Instant, animated: bool) -> Option<usize> {
        let Gesture::Dragging { velocity, .. } = self.gesture else {
            return None;
        };
        let (cur, _) = self.pos(now, step);
        self.gesture = Gesture::Idle;
        let target = settle(cur, velocity);
        self.pos = Transition::rest(cur);
        self.pos.go(target, Kind::Return, now, animated);
        self.selected = (target as i64).rem_euclid(self.len as i64) as usize;
        Some(self.selected)
    }
}

type Render = Rc<dyn Fn(usize, f32, &mut Window, &mut App) -> AnyElement>;
type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// [`carousel`] で作る。
#[derive(IntoElement)]
pub struct Carousel {
    id: &'static str,
    len: usize,
    selected: usize,
    shape: Shape,
    /// 項目を薄くする幕の色（置いた面の色）。`None` なら今のテーマの地。
    backdrop: Option<u32>,
    render: Render,
    on_select: OnSelect,
}

impl Carousel {
    /// 地（`background`）以外の面に置くとき、その面の色を渡す（薄い項目に重ねる幕の色になる）。
    pub fn backdrop(mut self, color: u32) -> Self {
        self.backdrop = Some(color);
        self
    }
}

/// `len` 個（1 以上）の項目を `shape` の形に並べ、`selected` 番目を中央に置く。`render` が項目 1 つ（番号と、一辺の長さ px の
/// 正方形）を作る。動かして収まる項目が変わる・中央でない項目を押すと `on_select` が呼ばれるので、呼び出し側は `selected` を
/// 変えて描き直す。幅は置いた所いっぱい、高さは `shape` で決まる。全体は `{id}`、項目は `{id}-item-<番号>` で指せる。
pub fn carousel(
    id: &'static str,
    len: usize,
    selected: usize,
    shape: Shape,
    render: impl Fn(usize, f32, &mut Window, &mut App) -> AnyElement + 'static,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Carousel {
    let len = len.max(1);
    Carousel {
        id,
        len,
        selected: selected.min(len - 1),
        shape,
        backdrop: None,
        render: Rc::new(render),
        on_select: Rc::new(on_select),
    }
}

impl RenderOnce for Carousel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Carousel {
            id,
            len,
            selected,
            shape,
            backdrop,
            render,
            on_select,
        } = self;
        let animated = cx.has_global::<Motion>();
        let now = crate::drive::now();
        let step = shape.step.max(1.0);
        let state = window.use_keyed_state(id, cx, move |_, _| State {
            pos: Transition::rest(selected as f32),
            selected,
            len,
            gesture: Gesture::Idle,
            bounds: Bounds::default(),
        });
        let (pos, moving, dragging) = state.update(cx, |s, _| {
            let dragging = matches!(s.gesture, Gesture::Dragging { .. });
            if s.len != len {
                // 項目が入れ替わった。前の並びからは回さない。
                (s.len, s.selected, s.gesture) = (len, selected, Gesture::Idle);
                s.pos = Transition::rest(selected as f32);
            } else if s.selected != selected && !dragging {
                // 外から選ぶ項目が変わった（左右のボタンなど）。近い向きに回る。
                let (cur, _) = s.pos.at(now);
                s.pos = Transition::rest(cur);
                s.pos
                    .go(cur + offset(selected, cur, len), Kind::Move, now, animated);
                s.selected = selected;
            }
            let (pos, moving) = s.pos(now, step);
            let dragging = matches!(s.gesture, Gesture::Dragging { .. });
            if !moving && !dragging {
                // 収まった。回った数は持ち越さない。
                s.pos = Transition::rest(s.selected as f32);
            }
            (pos, moving, dragging)
        });
        if moving {
            keep_drawing(window, cx);
        }

        // 範囲を覚え、ウィンドウ全体で入力を見る。最初の子なので、捕捉の段階では中身より先、
        // 浮き上がりの段階では中身より後（外側の要素よりは先）に呼ばれる。
        let watch = canvas(
            {
                let state = state.clone();
                move |bounds, _, cx| state.update(cx, |s, _| s.bounds = bounds)
            },
            {
                let on_select = on_select.clone();
                move |_, _, window, _| {
                    // 離した: 収まる項目が変わっていれば知らせる。
                    let release = {
                        let (st, on_select) = (state.clone(), on_select.clone());
                        move |window: &mut Window, cx: &mut App| {
                            let animated = cx.has_global::<Motion>();
                            let now = crate::drive::now();
                            let settled = st.update(cx, |s, _| s.release(step, now, animated));
                            if let Some(i) = settled.filter(|&i| i != selected) {
                                on_select(i, window, cx);
                            }
                            window.refresh();
                        }
                    };
                    let (s, lift) = (state.clone(), release.clone());
                    window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                        let ScrollDelta::Pixels(d) = e.delta else {
                            return;
                        };
                        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                        let gesture = s.read(cx).gesture;
                        match (phase, gesture, e.touch_phase) {
                            // 動かしている間・離した後の慣性は、中身にも外にも流さない。
                            (
                                DispatchPhase::Capture,
                                Gesture::Dragging { .. },
                                TouchPhase::Ended,
                            ) => {
                                cx.stop_propagation();
                                lift(window, cx);
                                s.update(cx, |s, _| s.gesture = Gesture::Coasting);
                            }
                            (DispatchPhase::Capture, Gesture::Dragging { .. }, _) => {
                                cx.stop_propagation();
                                s.update(cx, |s, _| {
                                    if let Gesture::Dragging {
                                        moved, velocity, ..
                                    } = &mut s.gesture
                                    {
                                        *moved += dx;
                                        *velocity = *velocity * 0.5 + dx * 0.5;
                                    }
                                });
                                window.refresh();
                            }
                            (DispatchPhase::Capture, Gesture::Coasting, TouchPhase::Ended) => {
                                cx.stop_propagation();
                            }
                            (DispatchPhase::Capture, Gesture::Coasting, _) => {
                                s.update(cx, |s, _| s.gesture = Gesture::Idle);
                            }
                            // 指のスライドの始まりが横向きなら受け取る（中の要素が止めていなければ）。
                            (DispatchPhase::Bubble, Gesture::Idle, TouchPhase::Started)
                                if dx.abs() > dy.abs()
                                    && s.read(cx).bounds.contains(&e.position) =>
                            {
                                cx.stop_propagation();
                                let now = crate::drive::now();
                                s.update(cx, |s, _| {
                                    s.gesture = Gesture::Dragging {
                                        from: s.pos.at(now).0,
                                        moved: dx,
                                        velocity: 0.0,
                                        start: None,
                                    }
                                });
                                window.refresh();
                            }
                            _ => {}
                        }
                    });
                    let st = state.clone();
                    window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                        let s = st.read(cx);
                        if phase == DispatchPhase::Bubble
                            && e.button == MouseButton::Left
                            && matches!(s.gesture, Gesture::Idle | Gesture::Coasting)
                            && s.bounds.contains(&e.position)
                        {
                            // 外側（画面の積み重ね）が、同じ押下でスワイプバックを始めないように。
                            cx.stop_propagation();
                            st.update(cx, |s, _| s.gesture = Gesture::Pressed(e.position));
                        }
                    });
                    let st = state.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                        // ボタンを押していない動きは別の手のもの（GPUI のマウスは 1 つ）。押しているときだけ見る。
                        if phase != DispatchPhase::Capture
                            || e.pressed_button != Some(MouseButton::Left)
                        {
                            return;
                        }
                        let x = f32::from(e.position.x);
                        match st.read(cx).gesture {
                            Gesture::Pressed(at) => {
                                let d = e.position - at;
                                let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                                if dx.hypot(dy) <= SLOP {
                                    return;
                                }
                                let now = crate::drive::now();
                                st.update(cx, |s, _| {
                                    s.gesture = if dx.abs() > dy.abs() {
                                        Gesture::Dragging {
                                            from: s.pos.at(now).0,
                                            moved: dx,
                                            velocity: 0.0,
                                            start: Some(f32::from(at.x)),
                                        }
                                    } else {
                                        // 横向きでない動きでは諦めず、ここを起点に置き直して横への動きを待つ（手ブレで最初の動きが横とは限らない）。
                                        Gesture::Pressed(e.position)
                                    }
                                });
                                window.refresh();
                            }
                            Gesture::Dragging {
                                moved,
                                start: Some(start),
                                ..
                            } => {
                                cx.stop_propagation();
                                let next = x - start;
                                st.update(cx, |s, _| {
                                    if let Gesture::Dragging {
                                        moved: m, velocity, ..
                                    } = &mut s.gesture
                                    {
                                        *velocity = *velocity * 0.5 + (next - moved) * 0.5;
                                        *m = next;
                                    }
                                });
                                window.refresh();
                            }
                            _ => {}
                        }
                    });
                    // 離した・押したまま外れた（その手でパネルを掴んだ）。
                    let lift = {
                        let st = state.clone();
                        move |window: &mut Window, cx: &mut App| match st.read(cx).gesture {
                            Gesture::Pressed(_) => st.update(cx, |s, _| s.gesture = Gesture::Idle),
                            Gesture::Dragging { start: Some(_), .. } => {
                                // 離した所の項目を押したことにしない。
                                cx.stop_propagation();
                                release(window, cx);
                            }
                            _ => {}
                        }
                    };
                    let up = lift.clone();
                    window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                            up(window, cx);
                        }
                    });
                    window.on_mouse_event(move |e: &MouseExitEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture
                            && e.pressed_button == Some(MouseButton::Left)
                        {
                            lift(window, cx);
                        }
                    });
                }
            },
        )
        .absolute()
        .size_full();

        // 見えている項目を、遠いものから重ねる（中央が一番上）。
        let mut shown: Vec<(usize, f32)> = (0..len)
            .map(|i| (i, offset(i, pos, len)))
            .filter(|(_, d)| d.abs() < shape.reach)
            .collect();
        shown.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        // 項目ごと薄くする: 要素ごとの不透明度では重なった下の部品が透けて見えるので、不透明なまま描いて地の色の円で覆う。
        // ponytail: 面が一色で項目が円のときだけ、項目ごと薄くしたのと同じに見える。地以外の面の色は [`Carousel::backdrop`] で渡す。
        let backdrop = backdrop.unwrap_or_else(|| crate::Theme::current(cx).background);
        let backdrop = Hsla::from(rgba(backdrop));
        let items = shown.into_iter().map(|(i, d)| {
            let place = shape.place(d);
            let on_select = on_select.clone();
            div()
                .id(ElementId::NamedInteger("carousel-item".into(), i as u64))
                .debug_selector(move || format!("{id}-item-{i}"))
                .absolute()
                .left(px(place.x - place.side / 2.0))
                .top(px(place.y - place.side / 2.0))
                .size(px(place.side))
                .child(Rotated {
                    angle: place.angle,
                    child: render(i, place.side, window, cx),
                })
                .when(place.alpha < 1.0, |item| {
                    item.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .rounded(px(place.side / 2.0))
                            .circular_corners()
                            .bg(backdrop.opacity(1.0 - place.alpha)),
                    )
                })
                // 中央でない項目は、押すと中央に来る。
                .when(i != selected, |item| {
                    item.on_click(move |_, window, cx| on_select(i, window, cx))
                })
        });
        div()
            .id(id)
            .debug_selector(move || id.into())
            .relative()
            .flex_none()
            .w_full()
            .h(px(shape.height))
            .child(watch)
            // 左右中央の上端を原点に、項目を置く（幅が分からなくても中央に並ぶ）。
            .child(
                div()
                    .absolute()
                    .left(relative(0.5))
                    .top_0()
                    .children(items.collect::<Vec<_>>()),
            )
            // 動かしている間は中身を覆う（離したときに項目の上でもクリックにしない・ホバーを消す）。
            .children(
                dragging.then(|| div().id("carousel-shield").absolute().size_full().occlude()),
            )
    }
}

/// 中身を自分の中心の周りに `angle`（ラジアン。時計回りが正）だけ回して描く。並べ方と押せる範囲は回さない。
struct Rotated {
    angle: f32,
    child: AnyElement,
}

impl IntoElement for Rotated {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Rotated {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
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
        self.child.prepaint(window, cx);
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
        let child = &mut self.child;
        window.with_rotation(bounds.center(), Radians(self.angle), |window| {
            child.paint(window, cx)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_wrap_around_the_ends() {
        assert_eq!(offset(0, 0.0, 9), 0.0);
        assert_eq!(offset(1, 0.0, 9), 1.0);
        assert_eq!(offset(8, 0.0, 9), -1.0, "last sits left of the first");
        assert_eq!(offset(0, 8.0, 9), 1.0, "first sits right of the last");
        // 回った数（通し番号）が増えても同じ並び。
        assert_eq!(offset(2, 9.0 * 3.0 + 1.0, 9), 1.0);
        assert_eq!(offset(0, -1.0, 3), 1.0);
        assert_eq!(offset(0, 0.25, 1), -0.25);
    }

    #[test]
    fn release_settles_on_the_nearest_item_or_the_one_flung_toward() {
        assert_eq!(settle(0.4, 0.0), 0.0);
        assert_eq!(settle(0.6, 0.0), 1.0);
        assert_eq!(settle(-0.6, 1.0), -1.0);
        // 右へ払うと項目は右へ流れ、前の項目（番号が小さい方）に収まる。左はその逆。
        assert_eq!(settle(-0.2, FLING), -1.0);
        assert_eq!(settle(0.2, -FLING), 1.0);
        assert_eq!(settle(2.0, FLING), 2.0, "動かしていなければそのまま");
    }

    #[test]
    fn released_drag_reports_the_wrapped_item() {
        let now = Instant::now();
        let drag = |moved: f32, velocity: f32| State {
            pos: Transition::rest(0.0),
            selected: 0,
            len: 3,
            gesture: Gesture::Dragging {
                from: 0.0,
                moved,
                velocity,
                start: None,
            },
            bounds: Bounds::default(),
        };
        // 項目 1 つ分は 100px。右へ 0.7 個分動かすと、端を越えて最後の項目。
        let mut s = drag(70.0, 0.0);
        assert_eq!(s.release(100.0, now, false), Some(2));
        assert_eq!(s.pos.at(now), (-1.0, false));
        assert_eq!(s.gesture, Gesture::Idle);
        let mut s = drag(-30.0, 0.0);
        assert_eq!(s.release(100.0, now, false), Some(0), "足りなければ戻る");
        assert_eq!(drag(-30.0, -FLING).release(100.0, now, false), Some(1));
        assert_eq!(s.release(100.0, now, false), None, "動かしていない");
    }
}
