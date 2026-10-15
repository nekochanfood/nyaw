//! Components: 役割（[`Theme`]）と Constants だけで組む部品。生の色は書かない。
//! 部品は `on_click` などをそのまま足せる（`Styled` / `InteractiveElement` を中の `Stateful<Div>` に委ねている）。
//! ホバー・押下の色は [`duration_ms::NORMAL`] かけて OKLab で補間する（[`Motion`] が有効なときだけ）。

use std::time::Instant;

use gpui::{
    App, Div, FontWeight, InteractiveElement, Interactivity, IntoElement, MouseButton, RenderOnce,
    SharedString, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
    prelude::*, px, rgba,
};
use palette::{FromColor, Mix, Oklab, Srgb};

use crate::{Motion, Theme, duration_ms, font_size, radius, spacing};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Primary,
    Danger,
}

/// 色の移り変わり: `from` から `to` へ `start` から動いている。`start` が `None` なら落ち着いている。
#[derive(Clone, Copy)]
struct Fade {
    from: u32,
    to: u32,
    start: Option<Instant>,
}

impl Fade {
    /// 今の色と、まだ動いているか。
    fn at(&self, now: Instant) -> (u32, bool) {
        let Some(start) = self.start else {
            return (self.to, false);
        };
        let t = now.saturating_duration_since(start).as_secs_f32() * 1000.0
            / duration_ms::NORMAL as f32;
        if t >= 1.0 {
            return (self.to, false);
        }
        // ease-out（立ち上がりが速く、着地がやわらかい）
        let e = 1.0 - (1.0 - t).powi(3);
        (mix(self.from, self.to, e), true)
    }
}

/// 0xRRGGBBAA を OKLab で混ぜる（アルファは混ぜない）。
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let lab = |c: u32| -> Oklab {
        let s: Srgb<f32> = Srgb::<u8>::from(c >> 8).into_format();
        Oklab::from_color(s)
    };
    let m = lab(a).mix(lab(b), t);
    let s: Srgb<u8> = Srgb::from_color(m).into_format();
    u32::from(s.red) << 24 | u32::from(s.green) << 16 | u32::from(s.blue) << 8 | (b & 0xff)
}

#[derive(Clone, Copy)]
struct State {
    pointer: Pointer,
    fade: Fade,
}

#[derive(Clone, Copy, Default)]
struct Pointer {
    hovered: bool,
    pressed: bool,
}

/// 押せるボタン。`id` はそのまま `debug_selector` にもなる。
#[derive(IntoElement)]
pub struct Button {
    id: &'static str,
    base: Stateful<Div>,
    rest: u32,
    hover: u32,
    active: u32,
}

impl Button {
    /// 切り替えボタンが有効なときの見た目（平常時の色が押下の色になる）。
    pub fn selected(mut self, selected: bool) -> Self {
        if selected {
            self.rest = self.active;
        }
        self
    }
}

pub fn button(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    variant: Variant,
) -> Button {
    let (bg, hover, active, fg) = match variant {
        Variant::Primary => (
            theme.accent_solid,
            theme.accent_solid_hover,
            theme.accent_solid_active,
            theme.on_accent,
        ),
        Variant::Danger => (
            theme.danger_solid,
            theme.danger_solid_hover,
            theme.danger_solid_active,
            theme.on_danger,
        ),
    };
    let base = div()
        .id(id)
        .debug_selector(move || id.into())
        .px(px(spacing::LG))
        .py(px(spacing::SM))
        .rounded(px(radius::MD))
        .text_size(px(font_size::MD))
        // 太字の 18.66px 以上は大きい文字（WCAG の 3:1）。ボタンの文字色の検査はこれを前提にしている。
        .font_weight(FontWeight::BOLD)
        .text_color(rgba(fg))
        .child(label.into());
    Button {
        id,
        base,
        rest: bg,
        hover,
        active,
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let rest = self.rest;
        let state = window.use_keyed_state(self.id, cx, move |_, _| State {
            pointer: Pointer::default(),
            fade: Fade {
                from: rest,
                to: rest,
                start: None,
            },
        });
        let State {
            pointer: p,
            fade: f,
        } = *state.read(cx);
        let target = if p.pressed {
            self.active
        } else if p.hovered {
            self.hover
        } else {
            rest
        };

        let now = Instant::now();
        let color = if f.to == target {
            let (c, moving) = f.at(now);
            if moving {
                keep_drawing(window, cx);
            }
            c
        } else if cx.has_global::<Motion>() {
            // 目標が変わった。今の色から新しい目標へ向けて動き直す。
            let (from, _) = f.at(now);
            state.update(cx, |s, _| {
                s.fade = Fade {
                    from,
                    to: target,
                    start: Some(now),
                }
            });
            keep_drawing(window, cx);
            from
        } else {
            state.update(cx, |s, _| {
                s.fade = Fade {
                    from: target,
                    to: target,
                    start: None,
                }
            });
            target
        };

        // ポインタの状態を書き換えて描き直す。
        let set = move |hovered: Option<bool>, pressed: Option<bool>| {
            let state = state.clone();
            move |window: &mut Window, cx: &mut App| {
                state.update(cx, |s, _| {
                    let p = &mut s.pointer;
                    if let Some(h) = hovered {
                        p.hovered = h;
                        if !h {
                            p.pressed = false;
                        }
                    }
                    if let Some(d) = pressed {
                        p.pressed = d;
                    }
                });
                window.refresh();
            }
        };
        let (over, leave) = (set(Some(true), None), set(Some(false), None));
        let (down, up) = (set(None, Some(true)), set(None, Some(false)));
        let up_out = set(None, Some(false));
        self.base
            .bg(rgba(color))
            .on_hover(move |h, w, cx| if *h { over(w, cx) } else { leave(w, cx) })
            .on_mouse_down(MouseButton::Left, move |_, w, cx| down(w, cx))
            .on_mouse_up(MouseButton::Left, move |_, w, cx| up(w, cx))
            .on_mouse_up_out(MouseButton::Left, move |_, w, cx| up_out(w, cx))
    }
}

/// 色が動いている間は次のフレームも描く。VR のフレームループにも伝える（[`Motion`]）。
fn keep_drawing(window: &mut Window, cx: &mut App) {
    if cx.has_global::<Motion>() {
        let id = window.window_handle().window_id();
        cx.global_mut::<Motion>().animating.insert(id);
    }
    window.request_animation_frame();
}

/// オン / オフのスイッチ。押したときの処理は呼び出し側が `on_click` で足す。
pub fn switch(theme: &Theme, id: &'static str, on: bool) -> Stateful<Div> {
    let (w, h, pad) = (44.0, 24.0, 2.0);
    let knob = h - pad * 2.0;
    let mut track = div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .w(px(w))
        .h(px(h))
        .px(px(pad))
        .rounded(px(radius::FULL))
        .bg(rgba(if on { theme.accent_solid } else { theme.track }));
    if on {
        track = track.justify_end();
    }
    track.child(
        div()
            .size(px(knob))
            .rounded(px(radius::FULL))
            .bg(rgba(if on { theme.on_accent } else { theme.panel })),
    )
}

/// `steps` 個の段に分けたスライダー（`value` は 0..=1）。段を押すとその段の値で `on_change` が呼ばれる。
/// 段は `{id}-{i}` で指せる。
// ponytail: ドラッグで連続に動かすのではなく段を押す方式。連続値が要るならポインタの位置から値を出す版にする。
pub fn slider(
    theme: &Theme,
    id: &'static str,
    value: f32,
    steps: usize,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    assert!(steps >= 2, "スライダーは 2 段以上");
    let on_change = std::rc::Rc::new(on_change);
    let filled = (value.clamp(0.0, 1.0) * (steps - 1) as f32).round() as usize;
    let mut row = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(spacing::XS))
        .h(px(24.0));
    for i in 0..steps {
        let v = i as f32 / (steps - 1) as f32;
        let on_change = on_change.clone();
        let name = format!("{id}-{i}");
        row = row.child(
            div()
                .id(SharedString::from(name.clone()))
                .debug_selector(move || name.clone())
                .w(px(20.0))
                .h(px(if i == filled { 24.0 } else { 8.0 }))
                .rounded(px(radius::SM))
                .bg(rgba(if i <= filled {
                    theme.accent_solid
                } else {
                    theme.track
                }))
                .on_click(move |_, w, cx| on_change(v, w, cx)),
        );
    }
    row
}

/// 画面やパネルの地。文字色も役割から決まる。
pub fn surface(theme: &Theme) -> Div {
    div()
        .font(crate::font::font())
        .bg(rgba(theme.neutral_base))
        .text_color(rgba(theme.neutral_text))
        .text_size(px(font_size::MD))
        .line_height(px(font_size::MD * 1.4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fade_runs_from_start_to_target_over_normal() {
        let t0 = Instant::now();
        let f = Fade {
            from: 0x000000ff,
            to: 0xffffffff,
            start: Some(t0),
        };
        assert_eq!(f.at(t0), (0x000000ff, true));
        let (mid, moving) = f.at(t0 + Duration::from_millis(duration_ms::NORMAL / 2));
        assert!(moving && mid != 0x000000ff && mid != 0xffffffff);
        assert_eq!(
            f.at(t0 + Duration::from_millis(duration_ms::NORMAL)),
            (0xffffffff, false)
        );
    }

    #[test]
    fn settled_fade_stays_put() {
        let f = Fade {
            from: 1,
            to: 0x3060c0ff,
            start: None,
        };
        assert_eq!(f.at(Instant::now()), (0x3060c0ff, false));
    }
}
