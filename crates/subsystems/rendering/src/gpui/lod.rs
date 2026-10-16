//! GPUI の論理座標を保ち、非表示ウィンドウの描画密度を変える。
use gpui::{App, Window};

/// LOD0〜3 の描画密度。ウィンドウの変更は次のイベント処理で反映する。
// shortcut: Windows DPI 経路は端数サイズの viewport を丸める。GPUI に独立した描画密度 API が追加されたら置き換える。
#[cfg(windows)]
pub fn set_render_lod(window: &mut Window, cx: &mut App, resolution: (u32, u32), lod: u8) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SendMessageW, WM_DPICHANGED,
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get();
    // resize コールバックが App を借り直せるよう、update_window を抜けてから送る。
    cx.foreground_executor()
        .spawn(async move {
            // SAFETY: メインスレッドで自分のウィンドウへ同期送信する。RECT は送信が終わるまで有効。
            unsafe {
                let hwnd = HWND(hwnd as _);
                let (mut outer, mut client) = (RECT::default(), RECT::default());
                if GetWindowRect(hwnd, &mut outer).is_err()
                    || GetClientRect(hwnd, &mut client).is_err()
                {
                    return;
                }
                let lod = lod.min(3);
                let dpi = 96u32 >> lod;
                let width = (resolution.0 >> lod).max(1) as i32;
                let height = (resolution.1 >> lod).max(1) as i32;
                outer.right = outer.left + width + (outer.right - outer.left) - client.right;
                outer.bottom = outer.top + height + (outer.bottom - outer.top) - client.bottom;
                SendMessageW(
                    hwnd,
                    WM_DPICHANGED,
                    Some(WPARAM((dpi | dpi << 16) as usize)),
                    Some(LPARAM(&outer as *const RECT as isize)),
                );
            }
        })
        .detach();
}

/// LOD0〜3 の描画密度。Linux の headless のウィンドウは拡大率を直接書き換え、すぐに反映する。
#[cfg(target_os = "linux")]
pub fn set_render_lod(window: &mut Window, cx: &mut App, resolution: (u32, u32), lod: u8) {
    let scale = 1.0 / (1u32 << lod.min(3)) as f32;
    let size = gpui::size(gpui::px(resolution.0 as f32), gpui::px(resolution.1 as f32));
    gpui_linux::with_render_scale(scale, || window.resize(size));
    window.bounds_changed(cx);
}
