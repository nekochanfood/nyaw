//! 任意の直交輪郭を持つキー。描画とヒット判定に同じ角丸の輪郭を使う。
use crate::{Theme, font_size};
use gpui::{
    App, Bounds, ClickEvent, Div, InteractiveElement, Interactivity, IntoElement, MouseButton,
    PathBuilder, Pixels, RenderOnce, SharedString, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, canvas, div, point, prelude::*, px, rgba,
};

type Position = (f32, f32);
type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, Default)]
struct State {
    bounds: Bounds<Pixels>,
    hover: bool,
    pressed: bool,
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

pub fn rounded_contains(outline: &[Position], radius: f32, at: Position) -> bool {
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
    let mut inside = false;
    for (index, &a) in polygon.iter().enumerate() {
        let b = polygon[(index + 1) % polygon.len()];
        if (a.1 > at.1) != (b.1 > at.1) && at.0 < (b.0 - a.0) * (at.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

#[derive(IntoElement)]
pub struct ShapedButton {
    id: SharedString,
    base: Stateful<Div>,
    outline: Vec<Position>,
    radius: f32,
    colors: [u32; 3],
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
            press.update(cx, |s, _| s.pressed = pressed);
            window.refresh();
        });
        let up = state.clone();
        base = base.on_mouse_up(MouseButton::Left, move |_, window, cx| {
            up.update(cx, |s, _| s.pressed = false);
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
    }
}
