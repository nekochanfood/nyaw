//! 縦に並んだ項目（通知カード）を左右スワイプで除去する。トーストと通知センター（ダッシュボード一覧）が使用する。
//! タッチ方式では押下したまま横スクロール、マウス方式ではドラッグで操作する。いずれも横方向の移動量に追従して項目が
//! フェードアウトし、リリース時に幅の [`DISMISS`] を超えていれば横へスライドアウト後に高さが折りたたまれ下の項目が
//! 滑らかに詰まる（折りたたみ完了時に [`Swipes::take_gone`] が一度だけ返す。除去は呼び出し側が行う）。閾値未満なら元に戻る。

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

/// リリース時にこの割合（項目幅に対する比率）以上横移動していれば除去する。
pub const DISMISS: f32 = 0.3;
/// マウス押下後にこの距離（px）移動するまではスワイプとして扱わない（押下意図の手ブレ防止）。
const SLOP: f32 = 8.0;
/// リリース後のスライドアウト・復帰と、スライドアウト後の高さ折りたたみの動き。
const SETTLE: Motion = Motion::Return;
/// スライドアウト済み項目の高さ折りたたみ動作（下の項目が詰まる）。ease-in-out で遷移する（終盤が速いと詰まり切る箇所でバウンスして見える）。
pub const COLLAPSE: Motion = Motion::Morph;
/// スワイプなしで除去する項目（[`Swipes::dismiss`]）のスライドアウト動作。静止状態から開始するため、ドラッグ継続の
/// [`SETTLE`] より長く、ease-in-out でスライドアウトする（短いと消滅が速すぎる）。
pub const FLICK: Motion = Motion::Morph;

/// 単一項目のスワイプ状態。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    /// マウス押下済み（未移動）。
    Pressed { id: u32, at: Point<Pixels> },
    /// ドラッグ中（`moved` px）。マウスの場合は押下位置の X 座標 `start` を基準に計測する。
    Dragging {
        id: u32,
        moved: f32,
        start: Option<f32>,
    },
    /// リリース済み: `from` から `to`（スライドアウトなら ±幅、復帰なら 0）へ遷移中。
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
    /// スライドアウト完了済み項目: スライドアウト先（px）と完了時刻（ここから高さを折りたたむ）。
    gone: HashMap<u32, (f32, Instant)>,
    /// スワイプなしで除去する項目（[`Swipes::dismiss`]）: スライドアウト開始時刻。
    flicks: HashMap<u32, Instant>,
    /// 項目の矩形（ウィンドウ px。前フレームで計測）。スワイプ開始位置の判定と幅の取得に使用する。
    boxes: HashMap<u32, Bounds<Pixels>>,
}

/// 項目 `id` の現在の表示状態（[`Swipes::look`]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// 水平オフセット（px）。
    pub offset: f32,
    /// 不透明度（移動量に比例してフェードアウト）。
    pub opacity: f32,
    /// 高さの残存率（1 = 通常、0 = 折りたたみ完了）。
    pub height: f32,
}

/// スワイプ状態（毎フレーム再配置されるウィンドウのイベントリスナーと共有する）。
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

    /// リリース処理: 幅の [`DISMISS`] を超えていればスライドアウト、未満なら元に復帰する。
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
    /// 項目 `id` の現在の表示状態を取得する。スライドアウト完了時にそこから高さの折りたたみを開始する。
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

    /// 時間駆動のアニメーション中か（次フレームも描画が必要）。ドラッグ中は入力のたびに再描画するため含めない
    /// （リリースイベントが未到着で残留しても、無限に描画し続けない）。
    pub fn busy(&self) -> bool {
        let s = self.0.borrow();
        !s.gone.is_empty()
            || !s.flicks.is_empty()
            || matches!(s.gesture, Some(Gesture::Settling { .. }))
    }

    /// 折りたたみ完了した項目を返す（一度だけ）。呼び出し側はこのフレーム以降、該当項目を描画しない。
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

    /// 項目 `id` をスワイプなしで除去する（一括消去時）。`start` から右へスライドアウトを開始し（スワイプリリースと同じ動作）、
    /// 完了後に高さを折りたたみ、折りたたみ完了時に [`Self::take_gone`] が返す。時刻をずらして呼べば順次スライドアウトする。
    pub fn dismiss(&self, id: u32, start: Instant) {
        let mut s = self.0.borrow_mut();
        if !s.gone.contains_key(&id) {
            s.flicks.entry(id).or_insert(start);
        }
    }

    /// 表示されなくなった項目の状態を破棄する。
    pub fn forget(&self, drawn: &[u32]) {
        let mut s = self.0.borrow_mut();
        s.gone.retain(|id, _| drawn.contains(id));
        s.flicks.retain(|id, _| drawn.contains(id));
        s.boxes.retain(|id, _| drawn.contains(id));
    }

    /// 項目 `id` の前フレームの矩形（未計測なら `None`）。
    pub fn bounds(&self, id: u32) -> Option<Bounds<Pixels>> {
        self.0.borrow().boxes.get(&id).copied()
    }

    /// 項目 `child` を現在の表示状態（水平オフセットとフェード。高さは呼び出し側が [`Look::height`] で折りたたむ）で包み、矩形を計測する。
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

    /// 今フレームのスワイプイベントリスナーをウィンドウに配置する（毎フレーム再配置。項目描画後に呼ぶ）。`panel` は
    /// 再描画対象パネル。
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
            // 新しいタッチスライドが開始されたのに前回分が残留している（リリースイベント未到着）場合は破棄する。
            if phase == DispatchPhase::Capture
                && e.touch_phase == TouchPhase::Started
                && matches!(s.gesture, Some(Gesture::Dragging { start: None, .. }))
            {
                s.gesture = None;
            }
            match (phase, s.gesture, e.touch_phase) {
                // タッチスライドの開始方向が水平なら、その項目のドラッグを開始する。
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
            // ボタン非押下中の移動は別の手によるもの（GPUI のマウスは 1 つ）。押下中のみ処理する。
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
                // リリース位置の項目へのクリックイベント伝播を防止する。
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

    /// 幅の 3 割超を移動してリリースするとその方向へスライドアウト後に高さが折りたたまれ、完了時に一度だけ返す。閾値未満なら復帰する。
    #[test]
    fn a_far_enough_swipe_slides_out_then_collapses() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let far = dragged(-150.0);
        far.0.borrow_mut().release(t);
        assert_eq!(far.look(7, t).offset, -150.0, "リリース位置から遷移開始");
        let out = ms(SETTLE.duration_ms() + 1);
        assert_eq!(far.look(7, out).offset, -400.0);
        assert_eq!(far.look(7, out).opacity, 0.0);
        assert!(far.busy(), "折りたたみ中");
        let half = far.look(7, out + Duration::from_millis(COLLAPSE.duration_ms() / 2));
        assert!(half.height > 0.0 && half.height < 1.0, "{half:?}");
        assert!(far.take_gone(out).is_empty(), "折りたたみ完了まで返さない");
        let end = out + Duration::from_millis(COLLAPSE.duration_ms() + 1);
        assert_eq!(far.take_gone(end), [7]);
        assert!(far.take_gone(end).is_empty(), "一度のみ返す");
        assert!(!far.busy());
        assert!(
            !dragged(80.0).busy(),
            "ドラッグ中は入力のたびに再描画するため、busy にしない"
        );
        let short = dragged(80.0);
        short.0.borrow_mut().release(t);
        assert_eq!(short.look(7, out).offset, 0.0);
        assert!(short.take_gone(end).is_empty());
        assert_eq!(short.look(3, end).offset, 0.0, "他の項目は影響を受けない");
    }
}
