//! カードを左右にスワイプして片付ける（その場で邪魔なときに。OS の通知センター・クイック設定の一覧からは消さない）。
//! タッチの方式では押したまま動かすと横のスクロールで届き、マウスの方式ではドラッグで届く。どちらも横に動かした分だけ
//! カードが付いてきて、離したときに幅の [`DISMISS`] を超えていれば横へ抜けて消え、超えていなければ元へ戻る。

use std::collections::HashMap;
use std::time::Instant;

use design::animation::Motion;
use gpui::{
    Bounds, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollDelta, ScrollWheelEvent, TouchPhase, WeakEntity, Window,
};

use crate::Toast;

/// 離したとき、これだけ（カードの幅に対する割合）横に動かしていれば片付ける。
pub(crate) const DISMISS: f32 = 0.3;
/// マウスで押してから、これだけ（px）動かすまではスワイプにしない（押すつもりの手ブレ）。
const SLOP: f32 = 8.0;

/// 1 枚のカードのスワイプ。
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

/// スワイプの状態。
#[derive(Debug, Default)]
pub(crate) struct Swipes {
    gesture: Option<Gesture>,
    /// 横へ抜けて片付けたカードと、抜けた先（消えていく間もそこに描く）。
    gone: HashMap<u32, f32>,
}

impl Swipes {
    /// カード `id` を横にずらす量（px）と、まだ動いているか。
    pub(crate) fn offset(&mut self, id: u32, now: Instant) -> (f32, bool) {
        if let Some(&x) = self.gone.get(&id) {
            return (x, false);
        }
        match self.gesture {
            Some(Gesture::Dragging { id: g, moved, .. }) if g == id => (moved, false),
            Some(Gesture::Settling {
                id: g,
                from,
                to,
                since,
            }) if g == id => {
                let motion = Motion::Return;
                let t = now.saturating_duration_since(since).as_secs_f32() * 1000.0
                    / motion.duration_ms() as f32;
                if t >= 1.0 {
                    if to != 0.0 {
                        self.gone.insert(id, to);
                    }
                    self.gesture = None;
                    return (to, false);
                }
                (from + (to - from) * motion.ease(t), true)
            }
            _ => (0.0, false),
        }
    }

    /// 横へ抜けきって片付けるカード（一度だけ返す）。
    pub(crate) fn take_gone(&mut self, shown: &[u32]) -> Vec<u32> {
        let gone: Vec<u32> = self
            .gone
            .keys()
            .copied()
            .filter(|id| shown.contains(id))
            .collect();
        gone
    }

    /// 並べなくなったカードを忘れる。
    pub(crate) fn forget(&mut self, drawn: &[u32]) {
        self.gone.retain(|id, _| drawn.contains(id));
    }

    /// 動かしている途中か（離すまで、カードを押したことにしない）。
    fn dragging(&self) -> bool {
        matches!(self.gesture, Some(Gesture::Dragging { .. }))
    }

    /// 離した: 幅 `width` の [`DISMISS`] を超えていれば横へ抜け、超えていなければ元へ戻る。
    fn release(&mut self, width: f32, now: Instant) {
        if let Some(Gesture::Dragging { id, moved, .. }) = self.gesture {
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
        } else if matches!(self.gesture, Some(Gesture::Pressed { .. })) {
            self.gesture = None;
        }
    }
}

/// 位置 `at` にあるカード。
fn card_at(boxes: &HashMap<u32, Bounds<Pixels>>, at: Point<Pixels>) -> Option<(u32, f32)> {
    boxes
        .iter()
        .find(|(_, b)| b.contains(&at))
        .map(|(&id, b)| (id, f32::from(b.size.width)))
}

/// このフレームのスワイプの受け口をウィンドウに置く（描くたびに置き直す）。押せる所（手首・ダッシュボード）でだけ呼ぶ。
pub(crate) fn listen(toast: WeakEntity<Toast>, window: &mut Window) {
    let t = toast.clone();
    window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
        let ScrollDelta::Pixels(d) = e.delta else {
            return;
        };
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        let _ = t.update(cx, |toast, cx| {
            let swipes = &mut toast.swipes;
            match (phase, swipes.gesture, e.touch_phase) {
                // 指のスライド（タッチの方式）の始まりが横向きなら、そのカードを動かす。
                (DispatchPhase::Bubble, None, TouchPhase::Started) if dx.abs() > dy.abs() => {
                    if let Some((id, _)) = card_at(&toast.boxes.borrow(), e.position) {
                        swipes.gesture = Some(Gesture::Dragging {
                            id,
                            moved: dx,
                            start: None,
                        });
                        cx.stop_propagation();
                    }
                }
                (
                    DispatchPhase::Capture,
                    Some(Gesture::Dragging {
                        id, start: None, ..
                    }),
                    phase,
                ) => {
                    cx.stop_propagation();
                    if phase == TouchPhase::Ended {
                        let width = width_of(&toast.boxes.borrow(), id);
                        swipes.release(width, Instant::now());
                    } else if let Some(Gesture::Dragging { moved, .. }) = &mut swipes.gesture {
                        *moved += dx;
                    }
                }
                _ => return,
            }
            ui::notify(toast.panel, cx);
        });
        window.refresh();
    });
    let t = toast.clone();
    window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        let _ = t.update(cx, |toast, _| {
            if toast.swipes.gesture.is_none()
                && let Some((id, _)) = card_at(&toast.boxes.borrow(), e.position)
            {
                toast.swipes.gesture = Some(Gesture::Pressed { id, at: e.position });
            }
        });
    });
    let t = toast.clone();
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
        // ボタンを押していない動きは別の手のもの（GPUI のマウスは 1 つ）。押しているときだけ見る。
        if phase != DispatchPhase::Capture || e.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let x = f32::from(e.position.x);
        let _ = t.update(cx, |toast, cx| {
            let swipes = &mut toast.swipes;
            match swipes.gesture {
                Some(Gesture::Pressed { id, at }) => {
                    let d = e.position - at;
                    let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                    if dx.hypot(dy) > SLOP && dx.abs() > dy.abs() {
                        swipes.gesture = Some(Gesture::Dragging {
                            id,
                            moved: dx,
                            start: Some(f32::from(at.x)),
                        });
                        ui::notify(toast.panel, cx);
                    }
                }
                Some(Gesture::Dragging {
                    id,
                    start: Some(start),
                    ..
                }) => {
                    cx.stop_propagation();
                    swipes.gesture = Some(Gesture::Dragging {
                        id,
                        moved: x - start,
                        start: Some(start),
                    });
                    ui::notify(toast.panel, cx);
                }
                _ => {}
            }
        });
        window.refresh();
    });
    window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture || e.button != MouseButton::Left {
            return;
        }
        let _ = toast.update(cx, |toast, cx| {
            if toast.swipes.dragging() {
                // 離した所のカードを押したことにしない。
                cx.stop_propagation();
            }
            let id = match toast.swipes.gesture {
                Some(Gesture::Dragging { id, .. } | Gesture::Pressed { id, .. }) => id,
                _ => return,
            };
            let width = width_of(&toast.boxes.borrow(), id);
            toast.swipes.release(width, Instant::now());
            ui::notify(toast.panel, cx);
        });
        window.refresh();
    });
}

fn width_of(boxes: &HashMap<u32, Bounds<Pixels>>, id: u32) -> f32 {
    boxes
        .get(&id)
        .map_or(1.0, |b| f32::from(b.size.width).max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn dragged(moved: f32) -> Swipes {
        Swipes {
            gesture: Some(Gesture::Dragging {
                id: 7,
                moved,
                start: None,
            }),
            ..Default::default()
        }
    }

    /// 幅の 3 割を超えて動かして離すと、その向きへ抜けて片付く。超えなければ元へ戻る。
    #[test]
    fn a_far_enough_swipe_dismisses_and_a_short_one_springs_back() {
        let t = Instant::now();
        let done = t + Duration::from_secs(1);
        let mut far = dragged(-150.0);
        far.release(400.0, t);
        assert_eq!(far.offset(7, t).0, -150.0, "離した所から動き出す");
        assert_eq!(far.offset(7, done), (-400.0, false));
        assert_eq!(far.take_gone(&[7]), [7]);
        let mut short = dragged(80.0);
        short.release(400.0, t);
        assert_eq!(short.offset(7, done), (0.0, false));
        assert!(short.take_gone(&[7]).is_empty());
        assert_eq!(
            short.offset(3, done),
            (0.0, false),
            "ほかのカードは動かない"
        );
    }
}
