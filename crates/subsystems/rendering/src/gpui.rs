//! GPUI の非表示描画先・読み戻し・描画密度。
use ::gpui::{
    App, AppContext, Bounds, Entity, Point, Render, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowOptions, px, size,
};
mod lod;
pub use lod::set_render_lod;

/// CPU に読み戻した UI の1フレーム（RGBA8、行間に詰め物なし）。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// ウィンドウを描き直し、画面に出さずにオフスクリーンへ描画して読み戻す。
/// 大きさはデバイスピクセル（論理サイズ × 拡大率）。
// ponytail: gpui の `render_to_image` は test-support フィーチャー限定かつ Windows のみ実装。
// 上流で一般公開されたらフィーチャーを外す。Linux は gpui_wgpu のヘッドレスレンダラ経由が次の一手。
pub fn capture(window: &mut Window, cx: &mut App) -> gpui::Result<Frame> {
    window.draw(cx).clear(cx);
    read_back(window)
}

/// 描き直し済みのシーンを GPU で描画して読み戻す（[`capture`] の後半。プロファイラが別々に測るため分けてある）。
pub fn read_back(window: &mut Window) -> gpui::Result<Frame> {
    let img = window.render_to_image()?;
    Ok(Frame {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

/// パネル用ウィンドウの設定。描画は VR（novr はエミュレーター）に送るので、デスクトップには出さない（非表示）。
pub fn panel_window_options(resolution: (u32, u32)) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(resolution.0 as f32), px(resolution.1 as f32)),
        })),
        show: false,
        focus: false,
        // OS の最小幅に、小さな LOD の描画先を広げられないよう枠を持たせない。
        kind: gpui::WindowKind::PopUp,
        window_min_size: Some(size(px(1.), px(1.))),
        // 既定（Opaque）だと白で塗ってから描くので、半透明の画面も不透明になる。
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

/// パネル用ウィンドウを開く。Windows の非表示ウィンドウのレンダーターゲットは
/// 1x1 のまま作られるので、明示的にリサイズして実寸にする（反映は非同期。最初の数フレームは 1x1）。
pub fn open_panel_window<V: Render>(
    cx: &mut App,
    resolution: (u32, u32),
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui::Result<WindowHandle<V>> {
    let window = cx.open_window(panel_window_options(resolution), build)?;
    let size = size(px(resolution.0 as f32), px(resolution.1 as f32));
    cx.update_window(window.into(), |_, window, _| window.resize(size))?;
    Ok(window)
}
