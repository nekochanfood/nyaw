//! WebView の非表示描画・読み戻し・描画密度を OS ごとに実装する。
mod capture;
mod lod;
pub use capture::{Image, capture};
pub use lod::resize;
use tauri::{WebviewWindowBuilder, Wry};

/// WebView2 の起動引数。wry の既定の `--disable-features` に、表示していないウィンドウを隠れていると判定して
/// 描画を止める CalculateNativeWinOcclusion を足す（非表示のまま描かせて撮るため）。
const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion";
/// パネルのウィンドウをデスクトップに出さない。どの OS も WebView 自身の撮影 API（`capture`）で撮る。
/// WebKitGTK は未マップ（非表示）のウィンドウを描かず撮れないので、表示したまま画面の外に置く。
// ponytail: Wayland ではアプリがウィンドウの位置を決められず画面に出てしまう。X11（XWayland 含む）のみ。
// GDK_BACKEND=x11 で起動するか、オフスクリーン描画（WPE 等）に移すのが次の一手。
#[cfg(target_os = "linux")]
fn headless<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App<Wry>>,
) -> WebviewWindowBuilder<'a, Wry, tauri::App<Wry>> {
    b.visible(true).position(-20000.0, -20000.0)
}

/// パネルのウィンドウをデスクトップに出さない。WebView2（CapturePreview）も WKWebView（takeSnapshot）も
/// 非表示のまま描いて撮れるので、一度も表示しない。
#[cfg(not(target_os = "linux"))]
fn headless<'a>(
    b: WebviewWindowBuilder<'a, Wry, tauri::App<Wry>>,
) -> WebviewWindowBuilder<'a, Wry, tauri::App<Wry>> {
    b.visible(false)
}

/// WebKitGTK の描画と読み戻しに必要なイベントを進める。
pub fn pump_events() {
    // WebKitGTK の描画・読み戻しは GLib のイベントで進む。溜まっている分を処理する。
    #[cfg(target_os = "linux")]
    for _ in 0..256 {
        if !gtk::events_pending() {
            break;
        }
        gtk::main_iteration_do(false);
    }
}

/// 非表示で読み戻せる WebView にする。透過・フォーカス等の表示方針は呼び出し側が指定する。
pub fn offscreen_window<'a>(
    builder: WebviewWindowBuilder<'a, Wry, tauri::App<Wry>>,
    resolution: (u32, u32),
) -> WebviewWindowBuilder<'a, Wry, tauri::App<Wry>> {
    // 起動引数は同じデータフォルダのすべての WebView で揃える。
    let builder = builder
        .inner_size(resolution.0 as f64, resolution.1 as f64)
        .min_inner_size(1.0, 1.0)
        .additional_browser_args(BROWSER_ARGS)
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled);
    // 枠を消すと Windows の WebView の描画領域が縮むため残す。
    headless(builder)
}

/// WebView の独自スキームの OS 差を吸収する。
pub fn page_url(name: &str) -> tauri::Url {
    let base = if cfg!(windows) {
        "http://nyaw.localhost"
    } else {
        "nyaw://localhost"
    };
    format!("{base}/{name}/index.html")
        .parse()
        .expect("page name is usable in a URL path")
}
