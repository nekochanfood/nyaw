//! スライダー。軸（既定は横、[`Slider::vertical`] で縦）に沿ってハンドルをつかんで動かす。値は 0..=1。
//!
//! - つかんだら、ポインタがスライダーやパネルの外へ出ても離すまで追従する（ウィンドウ全体で動きとボタンの離しを見る）。
//!   離したのに知らせが来なかったときは、ボタンを押していない動きが来た時点で離したとみなす。
//! - つかめるのはハンドルだけ（溝を押しても何も起きない）。値はポインタの絶対位置から決まり、相対的な動き量では動かさない。
//! - 指のスライド: 押してそのまま動かすとタッチはスクロールとして届く（届く位置は押した所に固定）。ハンドルの上で始まったものは
//!   長押しなしでハンドルを動かし、外側のスクロールやスクロールバーへは流さない。動き量は始点からの合計を指の位置とみなし、
//!   ハンドルが指に付いてくる（行き過ぎても指が戻れば戻る）。ハンドル以外の場所のスクロールはそのままページへ流れる。
//! - 埋まった側（始点からハンドルまで）は `accent_solid`、残りは色を付けない。

use gpui::{
    App, Bounds, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, RenderOnce, ScrollDelta, ScrollWheelEvent, TouchPhase, Window, canvas, div, prelude::*,
    px, rgba,
};
use std::rc::Rc;

use crate::{Theme, radius};

/// ハンドルの直径。
const HANDLE: f32 = 20.0;
/// 溝の太さ。
const GROOVE: f32 = 6.0;

#[derive(Clone, Copy, Default)]
struct State {
    /// 直近の描画での、スライダー全体の範囲。
    bounds: Bounds<Pixels>,
    /// つかんでいる（ハンドルの中心からポインタまでの px）。
    grab: Option<f32>,
    /// 指のスライド中: (始めたときのハンドルの中心, 始点からの指の移動量の合計)。
    slide: Option<(f32, f32)>,
    /// 今の値（同じフレームで続けて届く入力が、描き直し前の値に戻らないように持つ）。
    value: f32,
}

type OnChange = Rc<dyn Fn(f32, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Slider {
    id: &'static str,
    value: f32,
    vertical: bool,
    length: f32,
    /// 空いた側（まだ埋まっていない溝）。
    groove: u32,
    fill: u32,
    knob: u32,
    knob_active: u32,
    on_change: OnChange,
}

/// `value`（0..=1）を軸に沿って動かすスライダー。動かすたびに `on_change` が新しい値で呼ばれる。
/// 全体は `{id}`、ハンドルは `{id}-handle` で指せる。
pub fn slider(
    theme: &Theme,
    id: &'static str,
    value: f32,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Slider {
    Slider {
        id,
        value: value.clamp(0.0, 1.0),
        vertical: false,
        length: 200.0,
        groove: theme.groove,
        fill: theme.accent_solid,
        knob: theme.on_accent,
        knob_active: theme.accent_solid_active,
        on_change: Rc::new(on_change),
    }
}

impl Slider {
    /// 縦にする（値 0 が下）。
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// 軸に沿った長さ（px）。
    pub fn length(mut self, length: f32) -> Self {
        self.length = length.max(HANDLE * 2.0);
        self
    }
}

/// スライダーの始点（横は左端、縦は下端）から測ったポインタの位置。
fn along(b: &Bounds<Pixels>, vertical: bool, p: Point<Pixels>) -> f32 {
    if vertical {
        f32::from(b.origin.y + b.size.height - p.y)
    } else {
        f32::from(p.x - b.origin.x)
    }
}

/// ハンドルの中心が `along` にあるときの値。
fn value_at(length: f32, along: f32) -> f32 {
    ((along - HANDLE / 2.0) / (length - HANDLE)).clamp(0.0, 1.0)
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Slider {
            id,
            value,
            vertical,
            length,
            groove: groove_color,
            fill,
            knob,
            knob_active,
            on_change,
        } = self;
        let state = window.use_keyed_state(id, cx, |_, _| State::default());
        // つかんでいる間は自分の値が正（呼び出し側が反映するまでの間に戻らない）。
        let (shown, grabbed) = state.update(cx, |s, _| {
            if s.grab.is_none() {
                s.value = value;
            }
            (s.value, s.grab.is_some())
        });
        let pos = shown * (length - HANDLE);

        // 値を変える（変わったときだけ知らせる）。
        let set = {
            let (state, on_change) = (state.clone(), on_change.clone());
            move |v: f32, window: &mut Window, cx: &mut App| {
                let v = v.clamp(0.0, 1.0);
                if state.update(cx, |s, _| std::mem::replace(&mut s.value, v)) != v {
                    on_change(v, window, cx);
                }
                window.refresh();
            }
        };

        // ハンドルを掴む。ポインタがハンドルの中心からずれていた分は保つ（掴んだ瞬間に値は動かない）。
        let press = {
            let state = state.clone();
            move |e: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                let (b, v) = {
                    let s = state.read(cx);
                    (s.bounds, s.value)
                };
                let center = HANDLE / 2.0 + v * (length - HANDLE);
                let grab = along(&b, vertical, e.position) - center;
                state.update(cx, |s, _| s.grab = Some(grab));
                cx.stop_propagation();
            }
        };
        // 指のスライド（ハンドルの上で始まったもの）。ホイールの目盛りはページへ流す。
        let slide = {
            let (state, set) = (state.clone(), set.clone());
            move |e: &ScrollWheelEvent, window: &mut Window, cx: &mut App| {
                let ScrollDelta::Pixels(d) = e.delta else {
                    return;
                };
                // 始まりはハンドルの上だけ。届く位置は押した所に固定でハンドルは指から先へ動くので、始まった後は全体で受ける。
                let (b, v, sliding) = {
                    let s = state.read(cx);
                    (s.bounds, s.value, s.slide.is_some())
                };
                let center = HANDLE / 2.0 + v * (length - HANDLE);
                if !sliding && (along(&b, vertical, e.position) - center).abs() > HANDLE / 2.0 {
                    return;
                }
                cx.stop_propagation();
                match e.touch_phase {
                    TouchPhase::Started | TouchPhase::Moved => {
                        let d = f32::from(if vertical { -d.y } else { d.x });
                        let (base, moved) = state.update(cx, |s, _| {
                            let (base, moved) = s
                                .slide
                                .unwrap_or((HANDLE / 2.0 + s.value * (length - HANDLE), 0.0));
                            s.slide = Some((base, moved + d));
                            (base, moved + d)
                        });
                        set(value_at(length, base + moved), window, cx);
                    }
                    _ => state.update(cx, |s, _| s.slide = None),
                }
            }
        };

        let (w, h) = if vertical {
            (HANDLE, length)
        } else {
            (length, HANDLE)
        };
        let groove = (HANDLE - GROOVE) / 2.0;
        let mut track = div()
            .absolute()
            .rounded(px(radius::FULL))
            .bg(rgba(groove_color));
        let mut filled = div().absolute().rounded(px(radius::FULL)).bg(rgba(fill));
        let mut handle = div()
            .id(format!("{id}-handle"))
            .debug_selector({
                let n = format!("{id}-handle");
                move || n.clone()
            })
            .absolute()
            .size(px(HANDLE))
            .rounded(px(radius::FULL))
            .border_2()
            .border_color(rgba(if grabbed { knob_active } else { fill }))
            .bg(rgba(knob))
            .on_mouse_down(MouseButton::Left, press);
        if vertical {
            track = track.left(px(groove)).w(px(GROOVE)).top_0().bottom_0();
            filled = filled
                .left(px(groove))
                .w(px(GROOVE))
                .bottom_0()
                .h(px(pos + HANDLE / 2.0));
            handle = handle.left_0().bottom(px(pos));
        } else {
            track = track.top(px(groove)).h(px(GROOVE)).left_0().right_0();
            filled = filled
                .top(px(groove))
                .h(px(GROOVE))
                .left_0()
                .w(px(pos + HANDLE / 2.0));
            handle = handle.top_0().left(px(pos));
        }

        // 範囲を覚え、つかんでいる間はウィンドウのどこでもポインタに追従する（パネルの外でも維持）。
        let watch = {
            let state = state.clone();
            canvas(
                {
                    let state = state.clone();
                    move |bounds, _, cx| state.update(cx, |s, _| s.bounds = bounds)
                },
                move |_, _, window, _| {
                    let (s, set) = (state.clone(), set.clone());
                    window.on_mouse_event(move |e: &MouseMoveEvent, _, window, cx| {
                        let Some(grab) = s.read(cx).grab else { return };
                        if e.pressed_button != Some(MouseButton::Left) {
                            s.update(cx, |s, _| s.grab = None);
                            window.refresh();
                            return;
                        }
                        let b = s.read(cx).bounds;
                        let p = along(&b, vertical, e.position);
                        set(value_at(length, p - grab), window, cx);
                    });
                    let s = state.clone();
                    window.on_mouse_event(move |e: &MouseUpEvent, _, window, cx| {
                        if e.button == MouseButton::Left && s.read(cx).grab.is_some() {
                            s.update(cx, |s, _| s.grab = None);
                            window.refresh();
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        };

        div()
            .id(id)
            .debug_selector(move || id.into())
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .on_scroll_wheel(slide)
            .child(track)
            .child(filled)
            .child(handle)
            .child(watch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_spans_the_handle_travel() {
        // ハンドルの中心が端から半径だけ内側にあるとき、0 と 1。
        assert_eq!(value_at(200.0, HANDLE / 2.0), 0.0);
        assert_eq!(value_at(200.0, 200.0 - HANDLE / 2.0), 1.0);
        assert_eq!(value_at(200.0, -50.0), 0.0);
        assert_eq!(value_at(200.0, 900.0), 1.0);
        assert!((value_at(200.0, 100.0) - 0.5).abs() < 1e-6);
    }
}
