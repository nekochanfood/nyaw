//! 縦に並んだ項目（通知のカード）を左右にスワイプして片付ける。トーストと通知センター（ダッシュボードの一覧）が使う。
//! タッチの方式では押したまま動かすと横のスクロールで届き、マウスの方式ではドラッグで届く。どちらも横に動かした分だけ
//! 項目が付いてきて薄れ、離したときに幅の [`DISMISS`] を超えていれば横へ抜け、そのあと高さが畳まれて下の項目が
//! 滑らかに詰まる（畳みきったら [`Swipes::take_gone`] が一度だけ返す。片付けるのは呼ぶ側）。超えていなければ元へ戻る。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use design::animation::Motion;
use gpui::{
    AnyElement, App, Bounds, DispatchPhase, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, Window,
    canvas, div, prelude::*, px,
};
use kernel::PanelId;

/// 離したとき、これだけ（項目の幅に対する割合）横に動かしていれば片付ける。
pub const DISMISS: f32 = 0.3;
/// マウスで押してから、これだけ（px）動かすまではスワイプにしない（押すつもりの手ブレ）。
const SLOP: f32 = 8.0;
/// 離した後に横へ抜ける・戻る動きと、抜けた後に高さを畳む動き。
const SETTLE: Motion = Motion::Return;
/// 抜けた項目の高さを畳む動き（下が詰まる）。ゆっくり動き出してゆっくり止まる（終わり際に速いと、詰まりきる所で跳ねて見える）。
pub const COLLAPSE: Motion = Motion::Morph;
/// スワイプせずに片付ける項目（[`Swipes::dismiss`]）が横へ抜ける動き。止まった所から動き出すので、指で動かした続きの
/// [`SETTLE`] より長く、ゆっくり動き出してゆっくり抜ける（短いと消えるのが速すぎる）。
pub const FLICK: Motion = Motion::Morph;

/// 1 つの項目のスワイプ。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    /// マウスで押した（まだ動かしていない）。
    Pressed { id: u32, at: Point<Pixels> },
    /// 横に動かしている（`moved` px）。マウスなら押した所の横の位置 `start` から測る。
    Dragging {
        id: u32,
        moved: f32,
        start: Option<f32>,
    },
    /// 離した: `from` から `to`（横へ抜けるなら ±幅、戻るなら 0）へ動いている。
    Settling {
        id: u32,
        from: f32,
        to: f32,
        since: Instant,
    },
}

#[derive(Debug, Default)]
struct State {
    gesture: Option<Gesture>,
    /// 横へ抜けきった項目: 抜けた先（px）と、抜けきった時刻（そこから高さを畳む）。
    gone: HashMap<u32, (f32, Instant)>,
    /// スワイプせずに片付ける項目（[`Swipes::dismiss`]）: 横へ抜け始める時刻。
    flicks: HashMap<u32, Instant>,
    /// 項目の置かれた所（ウィンドウの px。前のフレームに測ったもの）。どの項目の上で始まったか・幅を見る。
    boxes: HashMap<u32, Bounds<Pixels>>,
}

/// 項目 `id` の今の様子（[`Swipes::look`]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// 横のずれ（px）。
    pub offset: f32,
    /// 不透明度（離れるほど薄い）。
    pub opacity: f32,
    /// 高さの残り（1 = そのまま、0 = 畳みきった）。
    pub height: f32,
}

/// スワイプの状態（描くたびに置き直すウィンドウの受け口と共有する）。
#[derive(Clone, Default)]
pub struct Swipes(Rc<RefCell<State>>);

fn progress(since: Instant, motion: Motion, now: Instant) -> f32 {
    now.saturating_duration_since(since).as_secs_f32() * 1000.0 / motion.duration_ms() as f32
}

impl State {
    fn width(&self, id: u32) -> f32 {
        self.boxes
            .get(&id)
            .map_or(1.0, |b| f32::from(b.size.width).max(1.0))
    }

    /// 離した: 幅の [`DISMISS`] を超えていれば横へ抜け、超えていなければ元へ戻る。
    fn release(&mut self, now: Instant) {
        match self.gesture {
            Some(Gesture::Dragging { id, moved, .. }) => {
                let width = self.width(id);
                let to = if moved.abs() > width * DISMISS {
                    width.copysign(moved)
                } else {
                    0.0
                };
                self.gesture = Some(Gesture::Settling {
                    id,
                    from: moved,
                    to,
                    since: now,
                });
            }
            Some(Gesture::Pressed { .. }) => self.gesture = None,
            _ => {}
        }
    }

    fn card_at(&self, at: Point<Pixels>) -> Option<u32> {
        self.boxes
            .iter()
            .find(|(id, b)| {
                b.contains(&at) && !self.gone.contains_key(id) && !self.flicks.contains_key(id)
            })
            .map(|(&id, _)| id)
    }
}

impl Swipes {
    /// 項目 `id` の今の様子。横へ抜けきったら、そこから高さを畳む。
    pub fn look(&self, id: u32, now: Instant) -> Look {
        let mut s = self.0.borrow_mut();
        let width = s.width(id);
        let look = |offset: f32, height: f32| Look {
            offset,
            opacity: 1.0 - (offset.abs() / width).min(1.0),
            height,
        };
        if let Some(&(x, since)) = s.gone.get(&id) {
            let t = progress(since, COLLAPSE, now).min(1.0);
            return look(x, 1.0 - COLLAPSE.ease(t));
        }
        if let Some(&start) = s.flicks.get(&id) {
            if now < start {
                return look(0.0, 1.0);
            }
            let t = progress(start, FLICK, now);
            if t < 1.0 {
                return look(width * FLICK.ease(t), 1.0);
            }
            s.flicks.remove(&id);
            s.gone.insert(id, (width, now));
            return look(width, 1.0);
        }
        match s.gesture {
            Some(Gesture::Dragging { id: g, moved, .. }) if g == id => look(moved, 1.0),
            Some(Gesture::Settling {
                id: g,
                from,
                to,
                since,
            }) if g == id => {
                let t = progress(since, SETTLE, now);
                if t < 1.0 {
                    return look(from + (to - from) * SETTLE.ease(t), 1.0);
                }
                s.gesture = None;
                if to != 0.0 {
                    s.gone.insert(id, (to, now));
                }
                look(to, 1.0)
            }
            _ => look(0.0, 1.0),
        }
    }

    /// 時間で動いている途中か（次のフレームも描く）。指やマウスで動かしている間は入力のたびに描き直すので含めない
    /// （離したことが届かないまま残っても、描き続けない）。
    pub fn busy(&self) -> bool {
        let s = self.0.borrow();
        !s.gone.is_empty()
            || !s.flicks.is_empty()
            || matches!(s.gesture, Some(Gesture::Settling { .. }))
    }

    /// 畳みきった項目（一度だけ返す。呼ぶ側は、このフレームからもう並べない）。
    pub fn take_gone(&self, now: Instant) -> Vec<u32> {
        let mut s = self.0.borrow_mut();
        let done: Vec<u32> = s
            .gone
            .iter()
            .filter(|(_, (_, since))| progress(*since, COLLAPSE, now) >= 1.0)
            .map(|(&id, _)| id)
            .collect();
        for id in &done {
            s.gone.remove(id);
            s.boxes.remove(id);
        }
        done
    }

    /// 項目 `id` をスワイプせずに片付ける（まとめて消すとき）。`start` に右へ抜け始め（スワイプして離したのと同じ動き）、
    /// 抜けきったら高さを畳み、畳みきったら [`Self::take_gone`] が返す。時刻をずらして呼べば順に抜ける。
    pub fn dismiss(&self, id: u32, start: Instant) {
        let mut s = self.0.borrow_mut();
        if !s.gone.contains_key(&id) {
            s.flicks.entry(id).or_insert(start);
        }
    }

    /// 並べなくなった項目を忘れる。
    pub fn forget(&self, drawn: &[u32]) {
        let mut s = self.0.borrow_mut();
        s.gone.retain(|id, _| drawn.contains(id));
        s.flicks.retain(|id, _| drawn.contains(id));
        s.boxes.retain(|id, _| drawn.contains(id));
    }

    /// 項目 `id` の前のフレームの大きさ（測っていなければ `None`）。
    pub fn bounds(&self, id: u32) -> Option<Bounds<Pixels>> {
        self.0.borrow().boxes.get(&id).copied()
    }

    /// 項目 `child` を、今の様子（横のずれと薄れ。高さは呼ぶ側が [`Look::height`] で畳む）で包み、置かれた所を測る。
    pub fn item(&self, id: u32, look: Look, child: impl IntoElement) -> AnyElement {
        let state = self.0.clone();
        div()
            .relative()
            .w_full()
            .child(
                canvas(
                    move |bounds, _, _| {
                        state.borrow_mut().boxes.insert(id, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .left(px(look.offset.round()))
                    .opacity(look.opacity)
                    .child(child),
            )
            .into_any_element()
    }

    /// このフレームのスワイプの受け口をウィンドウに置く（描くたびに置き直す。項目より後に描く所で呼ぶ）。`panel` は
    /// 描き直すパネル。
    pub fn listen(&self, panel: PanelId, window: &mut Window) {
        let redraw = move |window: &mut Window, cx: &mut App| {
            crate::redraw(panel, cx);
            window.refresh();
        };
        let state = self.0.clone();
        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
            let ScrollDelta::Pixels(d) = e.delta else {
                return;
            };
            let (dx, dy) = (f32::from(d.x), f32::from(d.y));
            let mut s = state.borrow_mut();
            // 新しい指のスライドが始まったのに前のものが残っている（離したことが届かなかった）なら、前のものは捨てる。
            if phase == DispatchPhase::Capture
                && e.touch_phase == TouchPhase::Started
                && matches!(s.gesture, Some(Gesture::Dragging { start: None, .. }))
            {
                s.gesture = None;
            }
            match (phase, s.gesture, e.touch_phase) {
                // 指のスライド（タッチの方式）の始まりが横向きなら、その項目を動かす。
                (DispatchPhase::Bubble, None, TouchPhase::Started) if dx.abs() > dy.abs() => {
                    let Some(id) = s.card_at(e.position) else {
                        return;
                    };
                    s.gesture = Some(Gesture::Dragging {
                        id,
                        moved: dx,
                        start: None,
                    });
                    cx.stop_propagation();
                }
                (DispatchPhase::Capture, Some(Gesture::Dragging { start: None, .. }), phase) => {
                    cx.stop_propagation();
                    if phase == TouchPhase::Ended {
                        s.release(Instant::now());
                    } else if let Some(Gesture::Dragging { moved, .. }) = &mut s.gesture {
                        *moved += dx;
                    }
                }
                _ => return,
            }
            drop(s);
            redraw(window, cx);
        });
        let state = self.0.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, _, _| {
            if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
                return;
            }
            let mut s = state.borrow_mut();
            if s.gesture.is_none()
                && let Some(id) = s.card_at(e.position)
            {
                s.gesture = Some(Gesture::Pressed { id, at: e.position });
            }
        });
        let state = self.0.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            // ボタンを押していない動きは別の手のもの（GPUI のマウスは 1 つ）。押しているときだけ見る。
            if phase != DispatchPhase::Capture || e.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let mut s = state.borrow_mut();
            match s.gesture {
                Some(Gesture::Pressed { id, at }) => {
                    let d = e.position - at;
                    let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                    if dx.hypot(dy) <= SLOP || dx.abs() <= dy.abs() {
                        return;
                    }
                    s.gesture = Some(Gesture::Dragging {
                        id,
                        moved: dx,
                        start: Some(f32::from(at.x)),
                    });
                }
                Some(Gesture::Dragging {
                    id,
                    start: Some(start),
                    ..
                }) => {
                    cx.stop_propagation();
                    s.gesture = Some(Gesture::Dragging {
                        id,
                        moved: f32::from(e.position.x) - start,
                        start: Some(start),
                    });
                }
                _ => return,
            }
            drop(s);
            redraw(window, cx);
        });
        let state = self.0.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture || e.button != MouseButton::Left {
                return;
            }
            let mut s = state.borrow_mut();
            match s.gesture {
                // 離した所の項目を押したことにしない。
                Some(Gesture::Dragging { .. }) => cx.stop_propagation(),
                Some(Gesture::Pressed { .. }) => {}
                _ => return,
            }
            s.release(Instant::now());
            drop(s);
            redraw(window, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};
    use std::time::Duration;

    fn dragged(moved: f32) -> Swipes {
        let s = Swipes::default();
        {
            let mut st = s.0.borrow_mut();
            st.boxes.insert(
                7,
                Bounds::new(point(px(0.0), px(0.0)), size(px(400.0), px(80.0))),
            );
            st.gesture = Some(Gesture::Dragging {
                id: 7,
                moved,
                start: None,
            });
        }
        s
    }

    /// 幅の 3 割を超えて動かして離すと、その向きへ抜けてから高さが畳まれ、畳みきったら一度だけ返る。超えなければ戻る。
    #[test]
    fn a_far_enough_swipe_slides_out_then_collapses() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let far = dragged(-150.0);
        far.0.borrow_mut().release(t);
        assert_eq!(far.look(7, t).offset, -150.0, "離した所から動き出す");
        let out = ms(SETTLE.duration_ms() + 1);
        assert_eq!(far.look(7, out).offset, -400.0);
        assert_eq!(far.look(7, out).opacity, 0.0);
        assert!(far.busy(), "まだ畳んでいる");
        let half = far.look(7, out + Duration::from_millis(COLLAPSE.duration_ms() / 2));
        assert!(half.height > 0.0 && half.height < 1.0, "{half:?}");
        assert!(far.take_gone(out).is_empty(), "畳みきるまでは返さない");
        let end = out + Duration::from_millis(COLLAPSE.duration_ms() + 1);
        assert_eq!(far.take_gone(end), [7]);
        assert!(far.take_gone(end).is_empty(), "一度だけ");
        assert!(!far.busy());
        assert!(
            !dragged(80.0).busy(),
            "動かしている間は入力のたびに描くので、描き続けない"
        );
        let short = dragged(80.0);
        short.0.borrow_mut().release(t);
        assert_eq!(short.look(7, out).offset, 0.0);
        assert!(short.take_gone(end).is_empty());
        assert_eq!(short.look(3, end).offset, 0.0, "ほかの項目は動かない");
    }
}
