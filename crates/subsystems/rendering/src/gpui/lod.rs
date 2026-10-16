//! GPUI の論理座標を保ち、非表示ウィンドウの描画密度を変える。
use gpui::{App, Window, px, size};

/// LOD0〜3 の描画密度。ウィンドウの拡大率を 1/2^lod にして、論理サイズ `resolution` のまま描く解像度だけを落とす。
/// 拡大率の口は vendor/ の gpui_windows・gpui_linux に足してある（`with_render_scale`）。
/// Windows は resize と同じく次のイベント処理で反映し、Linux の headless のウィンドウはすぐに反映する。
// shortcut: Windows は WM_SIZE で論理サイズを「描画先の大きさ ÷ 拡大率」で取り直すので、割り切れない大きさでは丸めがずれる。
pub fn set_render_lod(window: &mut Window, cx: &mut App, resolution: (u32, u32), lod: u8) {
    let scale = 1.0 / (1u32 << lod.min(3)) as f32;
    let logical = size(px(resolution.0 as f32), px(resolution.1 as f32));
    #[cfg(windows)]
    {
        let _ = cx;
        gpui_windows::with_render_scale(scale, || window.resize(logical));
    }
    #[cfg(target_os = "linux")]
    {
        gpui_linux::with_render_scale(scale, || window.resize(logical));
        // headless は resize を gpui に知らせないので、拡大率と論理サイズを取り込ませる。
        window.bounds_changed(cx);
    }
}
