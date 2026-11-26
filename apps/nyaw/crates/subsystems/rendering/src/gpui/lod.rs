//! GPUI の論理座標を維持したまま、非表示ウィンドウの描画密度を変更する。
use gpui::{App, Window, px, size};

/// LOD0〜3 の描画密度設定。ウィンドウのスケールを 1/2^lod にして、論理サイズ `resolution` を維持したまま描画解像度のみを低下させる。
/// スケール設定の API（`with_render_scale`）は vendor/ の gpui_windows（Windows）と `super::linux`（Linux）に追加してある。
/// どちらの OS でも、resize と同様に次のイベント処理時に反映される。
// shortcut: Windows は WM_SIZE で論理サイズを「描画先サイズ ÷ スケール」で再計算するため、割り切れないサイズでは丸め誤差が生じる。
pub fn set_render_lod(window: &mut Window, cx: &mut App, resolution: (u32, u32), lod: u8) {
    let scale = 1.0 / (1u32 << lod.min(3)) as f32;
    let logical = size(px(resolution.0 as f32), px(resolution.1 as f32));
    let _ = cx;
    #[cfg(windows)]
    gpui_windows::with_render_scale(scale, || window.resize(logical));
    #[cfg(target_os = "linux")]
    super::linux::with_render_scale(scale, || window.resize(logical));
}
