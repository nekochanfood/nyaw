//! パネル用ウィンドウの表示設定。
use std::sync::Arc;

use gpui::{
    App, Bounds, Entity, Point, Render, RenderImage, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowOptions, px, size,
};

/// RGBA の画素を GPUI の画像にする。使わなくなった画像はフレームループが解放する（[`design::images`]）。
pub fn render_image(width: u32, height: u32, rgba: Vec<u8>) -> Option<Arc<RenderImage>> {
    kernel::rendering::gpui::render_image(width, height, rgba).map(design::images::track)
}

/// パネル用ウィンドウの設定。描画は VR（novr はエミュレーター）へ送出するため、デスクトップ上には表示しない。
pub fn panel_window_options(resolution: (u32, u32)) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(resolution.0 as f32), px(resolution.1 as f32)),
        })),
        show: false,
        focus: false,
        // 既定値（Opaque）では白色で下塗りされるため、半透明パネルが不透明として描画される。
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

/// UI の表示設定を適用し、環境固有の描画先の生成を Subsystems に委譲する。
pub fn open_panel_window<V: Render>(
    cx: &mut App,
    resolution: (u32, u32),
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui::Result<WindowHandle<V>> {
    kernel::rendering::gpui::open_window(cx, panel_window_options(resolution), resolution, build)
}
