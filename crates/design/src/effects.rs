//! 背景の材質とポインタ光。文字は通常の経路で鮮明に描く。

use gpui::{
    App, Bounds, Div, Element, ElementId, GlobalElementId, InspectorElementId, InteractiveElement,
    Interactivity, IntoElement, ParentElement, Pixels, Point, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, WindowId,
};
use std::collections::HashMap;

/// Design が所有する見た目のフラグ。未設定時は従来の不透明な表示。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub translucent: bool,
    pub pointer_light: bool,
}

impl gpui::Global for Effects {}

struct EdgeStrength(f32);
impl gpui::Global for EdgeStrength {}

impl Effects {
    pub const DEFAULT_EDGE_STRENGTH: f32 = 2.;
    pub const MAX_EDGE_STRENGTH: f32 = 3.;

    /// 見た目を決めるための一時調整。保存せず、起動時は既定値へ戻す。
    pub fn edge_strength(cx: &App) -> f32 {
        cx.try_global::<EdgeStrength>()
            .map_or(Self::DEFAULT_EDGE_STRENGTH, |v| v.0)
    }

    pub fn set_edge_strength(value: f32, cx: &mut App) {
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(0., Self::MAX_EDGE_STRENGTH);
        if Self::edge_strength(cx) != value {
            cx.set_global(EdgeStrength(value));
            cx.refresh_windows();
        }
    }

    /// 本番の既定。各効果は独立に切り替えられる。
    pub const ENABLED: Self = Self {
        translucent: true,
        pointer_light: true,
    };

    pub fn current(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    /// 描画バックエンドに渡す材質。OS・ランタイムには依存しない。
    pub fn sync(window: &Window, cx: &App) {
        let effects = Self::current(cx);
        let id = window.window_handle().window_id();
        let scale = window.scale_factor();
        let pointers = cx
            .try_global::<SurfacePointers>()
            .and_then(|p| p.0.get(&id))
            .copied()
            .unwrap_or_default();
        paint::set(
            id.as_u64(),
            paint::Material {
                pointers: pointers.map(|p| {
                    p.map_or([-1e6; 2], |p| {
                        [f32::from(p.x) * scale, f32::from(p.y) * scale]
                    })
                }),
                translucent: u32::from(effects.translucent),
                light: u32::from(effects.pointer_light && pointers.iter().any(Option::is_some)),
                scale,
                edge_strength: Self::edge_strength(cx),
            },
        );
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
        paint::remove(window.as_u64());
    }
}

/// Div と同じように組める面。描画時のフラグと、呼び出し側が上書きした角丸も反映する。
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

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Surface {
    type RequestLayoutState = <Div as Element>::RequestLayoutState;
    type PrepaintState = <Div as Element>::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.base)
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        Element::source_location(&self.base)
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        Effects::sync(window, cx);
        self.base.extend(std::mem::take(&mut self.children));
        self.base.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.base.prepaint(id, inspector, bounds, state, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.base
            .paint(id, inspector, bounds, state, prepaint, window, cx)
    }
}
