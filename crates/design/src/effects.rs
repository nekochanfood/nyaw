//! 面の透過と、面の上を照らすポインタ光。文字や操作部品の不透明度は変えない。

use gpui::{
    App, Bounds, ContentMask, Div, InteractiveElement, Interactivity, IntoElement, ParentElement,
    Pixels, Point, RenderOnce, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    WindowId, canvas, div, fill, point, px, rgba, size,
};
use std::collections::HashMap;

/// Design が所有する見た目のフラグ。未設定時は従来の不透明な表示。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub translucent: bool,
    pub pointer_light: bool,
}

impl gpui::Global for Effects {}

impl Effects {
    /// 本番の既定。各効果は独立に切り替えられる。
    pub const ENABLED: Self = Self {
        translucent: true,
        pointer_light: true,
    };

    pub fn current(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    /// 開いている画面にも即座に反映する。
    pub fn set(self, cx: &mut App) {
        if Self::current(cx) != self {
            cx.set_global(self);
            cx.refresh_windows();
        }
    }
}

/// 左右のポインタをウィンドウごとに保持する。UI は入力の配送前に更新する。
#[derive(Default)]
pub struct SurfacePointers(HashMap<WindowId, [Option<Point<Pixels>>; 2]>);

impl gpui::Global for SurfacePointers {}

impl SurfacePointers {
    pub fn set(
        window: &mut Window,
        source: usize,
        position: Option<Point<Pixels>>,
        cx: &mut App,
    ) -> bool {
        let pointers = cx
            .default_global::<Self>()
            .0
            .entry(window.window_handle().window_id())
            .or_default();
        if pointers[source] != position {
            pointers[source] = position;
            if Effects::current(cx).pointer_light {
                window.refresh();
                return true;
            }
        }
        false
    }

    pub fn remove(window: WindowId, cx: &mut App) {
        if cx.has_global::<Self>() {
            cx.global_mut::<Self>().0.remove(&window);
        }
    }
}

/// Div と同じように組める面。描画時のフラグと、呼び出し側が上書きした角丸も反映する。
#[derive(IntoElement)]
pub struct Surface {
    base: Div,
    children: Vec<gpui::AnyElement>,
}

impl Surface {
    pub(super) fn new(base: Div) -> Self {
        Self {
            base: base.relative(),
            children: Vec::new(),
        }
    }
}

impl Styled for Surface {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Surface {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Surface {}

impl ParentElement for Surface {
    fn extend(&mut self, children: impl IntoIterator<Item = gpui::AnyElement>) {
        self.children.extend(children);
    }
}

impl RenderOnce for Surface {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let effects = Effects::current(cx);
        if effects.translucent {
            if let Some(gpui::Fill::Color(background)) = self.base.style().background.as_mut() {
                *background = background.opacity(0.86);
            }
        }
        let radii = &self.base.style().corner_radii;
        let corners = [
            radii.top_left,
            radii.top_right,
            radii.bottom_right,
            radii.bottom_left,
        ]
        .map(|r| r.map_or(0., |r| f32::from(r.to_pixels(window.rem_size()))));
        let pointers = cx
            .try_global::<SurfacePointers>()
            .and_then(|p| p.0.get(&window.window_handle().window_id()))
            .copied()
            .unwrap_or_default();
        let light = (effects.pointer_light && pointers.iter().any(Option::is_some)).then(|| {
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    paint_light(bounds, corners, pointers, window);
                },
            )
            .absolute()
            .inset_0()
            .size_full()
        });
        self.base.children(light).children(self.children)
    }
}

fn paint_light(
    bounds: Bounds<Pixels>,
    corners: [f32; 4],
    pointers: [Option<Point<Pixels>>; 2],
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let [tl, tr, br, bl] = super::components::clamp_corners(corners, width, height);
    let (top, bottom) = (tl.max(tr), bl.max(br));
    // 同心円を薄く重ね、中心ほど明るくする。影の余白や角の外には描かない。
    for center in pointers.into_iter().flatten() {
        let radius = 128.;
        let reach = Bounds::new(
            center - point(px(radius), px(radius)),
            size(px(radius * 2.), px(radius * 2.)),
        );
        if !bounds.intersects(&reach) {
            continue;
        }
        let mut paint = |mask: Bounds<Pixels>| {
            if !mask.intersects(&reach) {
                return;
            }
            window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
                for step in (1..=32).rev() {
                    let r = radius * step as f32 / 32.;
                    let circle =
                        Bounds::new(center - point(px(r), px(r)), size(px(r * 2.), px(r * 2.)));
                    window.paint_quad(
                        fill(circle, rgba(crate::scale::WHITE & !0xff | 2)).corner_radii(px(r)),
                    );
                }
            });
        };
        let at = |x, y, w, h| Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w), px(h)));
        paint(at(0., top, width, height - top - bottom));
        for (band, left, right, lower) in [(top, tl, tr, false), (bottom, bl, br, true)] {
            for row in 0..band.ceil() as u32 {
                let d = row as f32;
                let l = super::components::corner_inset(left, d + 0.5);
                let r = super::components::corner_inset(right, d + 0.5);
                paint(at(
                    l,
                    if lower { height - d - 1. } else { d },
                    width - l - r,
                    1.,
                ));
            }
        }
    }
}
