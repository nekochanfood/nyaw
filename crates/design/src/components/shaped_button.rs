//! 任意の直交輪郭を持つキー。描画とヒット判定に同じ角丸の輪郭を使う。
use crate::{Theme, font_size};
use gpui::{
    App, Bounds, ClickEvent, ContentMask, Div, InteractiveElement, Interactivity, IntoElement,
    MouseButton, PathBuilder, Pixels, Point, RenderOnce, SharedString, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, canvas, div, fill, point,
    prelude::*, px, rgba, size,
};
use std::time::Instant;

type Position = (f32, f32);
type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, Default)]
struct State {
    bounds: Bounds<Pixels>,
    hover: bool,
    pressed: bool,
    ripple: Option<(Point<Pixels>, Instant)>,
}

/// 各頂点の手前・頂点・直後。短い辺では角丸が隣と交差しない半径に収める。
pub fn rounded_corners(outline: &[Position], radius: f32) -> Vec<(Position, Position, Position)> {
    let mut corners = Vec::new();
    for (index, &vertex) in outline.iter().enumerate() {
        let before = outline[(index + outline.len() - 1) % outline.len()];
        let after = outline[(index + 1) % outline.len()];
        let a = ((before.0 - vertex.0).powi(2) + (before.1 - vertex.1).powi(2)).sqrt();
        let b = ((after.0 - vertex.0).powi(2) + (after.1 - vertex.1).powi(2)).sqrt();
        let r = radius.max(0.).min(a / 2.).min(b / 2.);
        let toward = |p: Position, d: f32| {
            if d == 0. {
                vertex
            } else {
                (
                    vertex.0 + (p.0 - vertex.0) * r / d,
                    vertex.1 + (p.1 - vertex.1) * r / d,
                )
            }
        };
        corners.push((toward(before, a), vertex, toward(after, b)));
    }
    corners
}

fn rounded_polygon(outline: &[Position], radius: f32) -> Vec<Position> {
    let mut polygon = Vec::new();
    for (start, control, end) in rounded_corners(outline, radius) {
        for step in 0..=16 {
            let t = step as f32 / 16.;
            let s = 1. - t;
            polygon.push((
                s * s * start.0 + 2. * s * t * control.0 + t * t * end.0,
                s * s * start.1 + 2. * s * t * control.1 + t * t * end.1,
            ));
        }
    }
    polygon
}

fn row_spans(polygon: &[Position], y: f32) -> Vec<(f32, f32)> {
    let mut crossings = Vec::new();
    for (index, &a) in polygon.iter().enumerate() {
        let b = polygon[(index + 1) % polygon.len()];
        if (a.1 > y) != (b.1 > y) {
            crossings.push((b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0);
        }
    }
    crossings.sort_by(f32::total_cmp);
    crossings
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| (p[0], p[1]))
        .collect()
}

pub fn rounded_contains(outline: &[Position], radius: f32, at: Position) -> bool {
    row_spans(&rounded_polygon(outline, radius), at.1)
        .into_iter()
        .any(|(left, right)| at.0 >= left && at.0 < right)
}

#[derive(IntoElement)]
pub struct ShapedButton {
    id: SharedString,
    base: Stateful<Div>,
    outline: Vec<Position>,
    radius: f32,
    colors: [u32; 3],
    ink: u32,
    selected: bool,
    click: Option<Click>,
    tooltip: Option<SharedString>,
    content: Option<gpui::AnyElement>,
}

pub fn shaped_button(
    theme: &Theme,
    id: impl Into<SharedString>,
    outline: Vec<Position>,
    radius: f32,
) -> ShapedButton {
    let id = id.into();
    let selector = id.clone();
    ShapedButton {
        id: id.clone(),
        base: div()
            .id(id)
            .debug_selector(move || selector.to_string())
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgba(theme.neutral_text))
            .text_size(px(font_size::MD))
            .font_weight(gpui::FontWeight::BOLD),
        outline,
        radius,
        colors: [theme.control, theme.control_hover, theme.control_active],
        ink: theme.neutral_text,
        selected: false,
        click: None,
        tooltip: None,
        content: None,
    }
}

impl ShapedButton {
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn on_click(
        mut self,
        click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click = Some(Box::new(click));
        self
    }
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
}
impl Styled for ShapedButton {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}
impl InteractiveElement for ShapedButton {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}
impl StatefulInteractiveElement for ShapedButton {}

impl RenderOnce for ShapedButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State::default());
        let current = *state.read(cx);
        let color = self.colors[if current.pressed || self.selected {
            2
        } else if current.hover {
            1
        } else {
            0
        }];
        let outline = self.outline.clone();
        let radius = self.radius;
        let animated = cx.has_global::<crate::Motion>();
        let ripple = current.ripple.filter(|_| animated).filter(|(_, start)| {
            super::ripple_frame(start.elapsed().as_secs_f32() * 1000., 0.).is_some()
        });
        if ripple.is_some() {
            super::keep_drawing(window, cx);
        }
        let ink = self.ink;
        let paint_state = state.clone();
        let mut base = self.base.child(
            canvas(
                move |bounds, _, cx| paint_state.update(cx, |s, _| s.bounds = bounds),
                move |bounds, _, window, _| {
                    let mut path = PathBuilder::fill();
                    let at = |(x, y): Position| bounds.origin + point(px(x), px(y));
                    let corners = rounded_corners(&outline, radius);
                    if let Some((start, _, _)) = corners.first() {
                        path.move_to(at(*start));
                    }
                    for (start, control, end) in corners {
                        path.line_to(at(start));
                        path.curve_to(at(end), at(control));
                    }
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgba(color));
                    }
                    if let Some((origin, start)) = ripple {
                        let local = origin - bounds.origin;
                        let reach = outline
                            .iter()
                            .map(|&(x, y)| (x - f32::from(local.x)).hypot(y - f32::from(local.y)))
                            .fold(0., f32::max);
                        if let Some((r, alpha)) =
                            super::ripple_frame(start.elapsed().as_secs_f32() * 1000., reach)
                        {
                            let circle = Bounds::new(
                                origin - point(px(r), px(r)),
                                size(px(2. * r), px(2. * r)),
                            );
                            let quad = fill(circle, rgba(ink & !0xff | u32::from(alpha)))
                                .corner_radii(px(r));
                            let polygon = rounded_polygon(&outline, radius);
                            // 長方形のマスクを 1px の帯に分け、段付き Enter の凹みでも波紋を輪郭内に収める。
                            let top = (f32::from(local.y) - r).floor().max(0.) as u32;
                            let bottom = (f32::from(local.y) + r)
                                .ceil()
                                .min(bounds.size.height.into())
                                as u32;
                            for y in top..bottom {
                                for (left, right) in row_spans(&polygon, y as f32 + 0.5) {
                                    let mask = Bounds::new(
                                        bounds.origin + point(px(left), px(y as f32)),
                                        size(px(right - left), px(1.)),
                                    );
                                    window.with_content_mask(
                                        Some(ContentMask { bounds: mask }),
                                        |window| window.paint_quad(quad.clone()),
                                    );
                                }
                            }
                        }
                    }
                },
            )
            .absolute()
            .inset_0(),
        );
        if let Some(content) = self.content {
            base = base.child(content);
        }
        let outline = self.outline.clone();
        let hover_state = state.clone();
        let id = self.id.clone();
        let tip = self.tooltip.clone();
        // ツールチップの持ち主（要素が描かれなくなったら要求を消す。`Tooltips::drop_stale`）。
        let owner = state.clone().into_any().downgrade();
        base = base.on_mouse_move(move |event, window, cx| {
            let bounds = hover_state.read(cx).bounds;
            let at = event.position - bounds.origin;
            let hover = rounded_contains(&outline, radius, (at.x.into(), at.y.into()));
            if hover_state.read(cx).hover != hover {
                hover_state.update(cx, |s, _| s.hover = hover);
                window.refresh();
            }
            if let Some(tip) = &tip {
                super::hover_tooltip(id.clone(), tip.clone(), owner.clone(), hover, window, cx);
            }
        });
        let leave = state.clone();
        let id = self.id;
        let tip = self.tooltip;
        let owner = state.clone().into_any().downgrade();
        base = base.on_hover(move |hover, window, cx| {
            if !hover {
                leave.update(cx, |s, _| {
                    s.hover = false;
                    s.pressed = false;
                });
                if let Some(tip) = &tip {
                    super::hover_tooltip(id.clone(), tip.clone(), owner.clone(), false, window, cx);
                }
                window.refresh();
            }
        });
        let press = state.clone();
        let outline = self.outline.clone();
        base = base.on_mouse_down(MouseButton::Left, move |event, window, cx| {
            let at = event.position - press.read(cx).bounds.origin;
            let pressed = rounded_contains(&outline, radius, (at.x.into(), at.y.into()));
            press.update(cx, |s, _| {
                s.pressed = pressed;
                if pressed && animated {
                    s.ripple = Some((event.position, Instant::now()));
                }
            });
            window.refresh();
        });
        let up = state.clone();
        base = base.on_mouse_up(MouseButton::Left, move |_, window, cx| {
            up.update(cx, |s, _| s.pressed = false);
            window.refresh();
        });
        let up_out = state.clone();
        base = base.on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            up_out.update(cx, |s, _| s.pressed = false);
            window.refresh();
        });
        if let Some(click) = self.click {
            let outline = self.outline;
            base = base.on_click(move |event, window, cx| {
                let at = event.position() - state.read(cx).bounds.origin;
                if rounded_contains(&outline, radius, (at.x.into(), at.y.into())) {
                    click(event, window, cx);
                }
            });
        }
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sample;
    impl Render for Sample {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div().p(px(20.)).child(
                shaped_button(
                    &Theme::DARK,
                    "stepped",
                    vec![
                        (0., 0.),
                        (88., 0.),
                        (88., 120.),
                        (16., 120.),
                        (16., 56.),
                        (0., 56.),
                    ],
                    8.,
                )
                .w(px(88.))
                .h(px(120.)),
            )
        }
    }

    #[gpui::test]
    fn ripple_starts_on_the_key_but_not_in_the_enter_notch(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_global(crate::Motion::default()));
        let (_, cx) = cx.add_window_view(|_, _| Sample);
        cx.run_until_parked();
        let origin = cx.debug_bounds("stepped").unwrap().origin;
        cx.simulate_mouse_down(
            origin + point(px(4.), px(90.)),
            MouseButton::Left,
            gpui::Modifiers::none(),
        );
        cx.run_until_parked();
        assert!(cx.update(|_, cx| cx.global::<crate::Motion>().animating.is_empty()));
        cx.simulate_mouse_up(
            origin + point(px(4.), px(90.)),
            MouseButton::Left,
            gpui::Modifiers::none(),
        );
        cx.simulate_mouse_down(
            origin + point(px(50.), px(90.)),
            MouseButton::Left,
            gpui::Modifiers::none(),
        );
        cx.run_until_parked();
        assert!(cx.update(|_, cx| !cx.global::<crate::Motion>().animating.is_empty()));
    }
    #[test]
    fn corners_and_notch_use_the_same_rounded_outline() {
        let outline = [
            (0., 0.),
            (88., 0.),
            (88., 120.),
            (16., 120.),
            (16., 56.),
            (0., 56.),
        ];
        for radius in [0., 4., 12., 40.] {
            assert!(rounded_contains(&outline, radius, (50., 90.)));
            assert!(!rounded_contains(&outline, radius, (4., 90.)));
        }
        assert!(rounded_contains(&outline, 0., (1., 1.)));
        assert!(!rounded_contains(&outline, 12., (1., 1.)));
        assert!(rounded_contains(&outline, 12., (16., 16.)));
        let polygon = rounded_polygon(&outline, 12.);
        assert_eq!(row_spans(&polygon, 30.), [(0., 88.)]);
        assert_eq!(row_spans(&polygon, 90.), [(16., 88.)]);
        assert!(row_spans(&polygon, 1.)[0].0 > 0.);
    }
}
