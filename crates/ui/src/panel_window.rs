//! パネル用ウィンドウの表示方針。
use gpui::{
    App, Bounds, Entity, Point, Render, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowOptions, px, size,
};

/// パネル用ウィンドウの設定。描画は VR（novr はエミュレーター）に送るので、デスクトップには出さない（非表示）。
pub fn panel_window_options(resolution: (u32, u32)) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(resolution.0 as f32), px(resolution.1 as f32)),
        })),
        show: false,
        focus: false,
        // 既定（Opaque）だと白で塗ってから描くので、半透明の画面も不透明になる。
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

/// UI の表示方針を決め、環境ごとの描画先の作成は Subsystems に委譲する。
pub fn open_panel_window<V: Render>(
    cx: &mut App,
    resolution: (u32, u32),
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui::Result<WindowHandle<V>> {
    kernel::rendering::gpui::open_window(cx, panel_window_options(resolution), resolution, build)
}
