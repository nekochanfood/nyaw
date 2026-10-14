//! ページの論理座標を保ち、WebView の描画密度とウィンドウを同じ比率で縮める。
use tauri::{PhysicalSize, WebviewWindow, Wry};

pub fn resize(window: &WebviewWindow<Wry>, resolution: (u32, u32), lod: u8) -> tauri::Result<()> {
    let factor = 1.0 / f64::from(1u32 << lod);
    #[cfg(windows)]
    window.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller3;
        use windows::core::Interface;
        // SAFETY: Tauri のメインスレッド上で、生存中の WebView のコントローラを操作する。
        let result = unsafe {
            webview
                .controller()
                .cast::<ICoreWebView2Controller3>()
                .and_then(|controller| {
                    controller.SetShouldDetectMonitorScaleChanges(false)?;
                    // Chromium は描画密度を 0.5 以上に丸めるため、残りをページの zoom で補う。
                    controller.SetRasterizationScale(factor.max(0.5))?;
                    controller.SetZoomFactor(factor / factor.max(0.5))
                })
        };
        if let Err(e) = result {
            logger::error!("cannot change WebView render density", error = e);
        }
    })?;
    #[cfg(not(windows))]
    window.set_zoom(factor / window.scale_factor()?)?;
    window.set_size(PhysicalSize::new(
        (resolution.0 >> lod).max(1),
        (resolution.1 >> lod).max(1),
    ))
}
