//! ドラッグの見た目（[`DragPreview`]）を、押している手のドラッグ用のパネルにだけ描く。

use super::*;

/// ドラッグの見た目を描くウィンドウ（[`Pointer::Drag`] のパネル）。無ければ（novr）どのウィンドウでも描く。
#[derive(Default)]
pub(crate) struct DragWindows(pub(crate) Vec<AnyWindowHandle>);

impl gpui::Global for DragWindows {}

/// ドラッグ中の見た目（`on_drag` が返すビュー）を包む。GPUI はこれをどのウィンドウにもマウス位置に描くが、
/// VR ではカーソル側のウィンドウ（[`Pointer::Drag`]）だけに描く。パネルの外へ出てもウィンドウをまたいで見える。
pub struct DragPreview(AnyView);

/// `.on_drag(value, |_, _, _, cx| ui::drag_preview(cx, MyPreview))` のように使う。
pub fn drag_preview<V: Render>(cx: &mut App, view: V) -> Entity<DragPreview> {
    let view = cx.new(|_| view).into();
    cx.new(|_| DragPreview(view))
}

impl Render for DragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let here = window.window_handle();
        let shown = cx
            .try_global::<DragWindows>()
            .is_none_or(|d| d.0.is_empty() || d.0.contains(&here));
        div().when(shown, |d| d.child(self.0.clone()))
    }
}

/// `panel` で始まったドラッグの見た目を、押している手の [`Pointer::Drag`] のパネルに出す。
/// 大きさは元のパネルと同じ縮尺（1m あたりのピクセル）にする。
pub(crate) fn start_drag(
    kernel: &mut Kernel,
    bridges: &HashMap<(PanelId, Hand), PointerBridge>,
    windows: &[(PanelId, AnyWindowHandle)],
    panel: PanelId,
) -> Option<(Hand, PanelId)> {
    let hand = bridges
        .iter()
        .find(|((p, _), b)| *p == panel && b.pressed)
        .map(|((_, h), _)| *h)?;
    let (dp, _) = windows.iter().find(|(id, _)| {
        !kernel.panel(*id).closed && kernel.panel(*id).pointer == Some((hand, Pointer::Drag))
    })?;
    let src = kernel.panel(panel);
    let px_per_m = src.resolution.0 as f32 / src.size.x;
    let width = kernel.panel(*dp).resolution.0 as f32 / px_per_m;
    kernel.set_drag(hand, Some(width));
    Some((hand, *dp))
}
