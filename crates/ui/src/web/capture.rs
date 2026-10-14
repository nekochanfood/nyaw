//! WebView を撮って RGBA8（ストレートアルファ、行間に詰め物なし）で読み戻す。
//! どのプラットフォームも非同期 API で、結果は `done` に 1 回だけ届く（メインスレッドのイベントループ上で呼ばれる）。
//! UI 層の `ui::capture`（GPUI の `render_to_image`）に当たる。

use tauri::{WebviewWindow, Wry};

/// 読み戻した 1 フレーム（RGBA8、ストレートアルファ、行間に詰め物なし）。
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub type Done = Box<dyn FnOnce(Result<Image, String>) + Send>;

/// `window` の WebView を撮る。撮れなければ `done` にエラーが届く。
pub fn capture(window: &WebviewWindow<Wry>, done: Done) {
    // with_webview が失敗したとき（ウィンドウが消えた等）にも `done` を呼べるよう、取り出せる形で渡す。
    let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(done)));
    let inner = slot.clone();
    let sent = window.with_webview(move |webview| {
        if let Some(done) = inner.lock().ok().and_then(|mut d| d.take()) {
            platform::snapshot(webview, done);
        }
    });
    if let Err(e) = sent
        && let Some(done) = slot.lock().ok().and_then(|mut d| d.take())
    {
        done(Err(e.to_string()));
    }
}

/// cairo の ARGB32（ネイティブエンディアンの 0xAARRGGBB、乗算済みアルファ）を RGBA8 のストレートアルファにする。
/// `alpha` が偽なら RGB24（上位 8bit は無視して不透明）。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn from_argb32(data: &[u8], width: u32, height: u32, stride: usize, alpha: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        let row = &data[y * stride..y * stride + width as usize * 4];
        for px in row.as_chunks::<4>().0 {
            let v = u32::from_ne_bytes(*px);
            let a = if alpha { (v >> 24) as u8 } else { 255 };
            let [r, g, b] = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
            out.extend_from_slice(&match a {
                0 => [0, 0, 0, 0],
                255 => [r, g, b, 255],
                a => [
                    unpremultiply(r, a),
                    unpremultiply(g, a),
                    unpremultiply(b, a),
                    a,
                ],
            });
        }
    }
    out
}

fn unpremultiply(c: u8, a: u8) -> u8 {
    ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
}

#[cfg(target_os = "linux")]
mod platform {
    use tauri::webview::PlatformWebview;
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt, gio};

    use super::Image;
    use super::{Done, from_argb32};

    pub fn snapshot(webview: PlatformWebview, done: Done) {
        webview.inner().snapshot(
            SnapshotRegion::Visible,
            SnapshotOptions::TRANSPARENT_BACKGROUND,
            None::<&gio::Cancellable>,
            move |r| done(r.map_err(|e| e.to_string()).and_then(read)),
        );
    }

    fn read(surface: cairo::Surface) -> Result<Image, String> {
        let mut img = cairo::ImageSurface::try_from(surface)
            .map_err(|s| format!("画像のサーフェスではない: {:?}", s.type_()))?;
        img.flush();
        let (width, height) = (img.width() as u32, img.height() as u32);
        let stride = img.stride() as usize;
        let alpha = match img.format() {
            cairo::Format::ARgb32 => true,
            cairo::Format::Rgb24 => false,
            f => return Err(format!("扱えない画素形式: {f:?}")),
        };
        let data = img.data().map_err(|e| e.to_string())?;
        Ok(Image {
            width,
            height,
            rgba: from_argb32(&data, width, height, stride, alpha),
        })
    }
}

// ponytail: 実機（Windows）では未確認。非表示ウィンドウの WebView2 が CapturePreview で描けない場合は、
// コントローラを見える状態にしたままウィンドウだけ隠す（画面外に置く）のが次の一手。
#[cfg(windows)]
mod platform {
    use std::sync::{Arc, Mutex};

    use tauri::webview::PlatformWebview;
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};

    use super::Done;
    use super::Image;

    pub fn snapshot(webview: PlatformWebview, done: Done) {
        let slot = Arc::new(Mutex::new(Some(done)));
        let finish = {
            let slot = slot.clone();
            move |r: Result<Image, String>| {
                if let Some(done) = slot.lock().ok().and_then(|mut d| d.take()) {
                    done(r);
                }
            }
        };
        let started = (|| -> windows::core::Result<()> {
            // SAFETY: WebView2 の COM 呼び出し。with_webview の中（メインスレッド）で行う。
            let core = unsafe { webview.controller().CoreWebView2()? };
            let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true)? };
            let png = stream.clone();
            let finish = finish.clone();
            let handler = CapturePreviewCompletedHandler::create(Box::new(move |r| {
                finish(r.map_err(|e| e.to_string()).and_then(|_| read(&png)));
                Ok(())
            }));
            unsafe {
                core.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &stream,
                    &handler,
                )
            }
        })();
        if let Err(e) = started {
            finish(Err(e.to_string()));
        }
    }

    /// CapturePreview が書いた PNG を読み出して RGBA8 にする。
    fn read(stream: &IStream) -> Result<Image, String> {
        let mut bytes = Vec::new();
        let mut buf = [0u8; 64 * 1024];
        // SAFETY: buf の長さだけ書かせる。
        unsafe {
            stream
                .Seek(0, STREAM_SEEK_SET, None)
                .map_err(|e| e.to_string())?;
            loop {
                let mut n = 0u32;
                stream
                    .Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut n))
                    .ok()
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n as usize]);
            }
        }
        let mut decoder = png::Decoder::new(bytes.as_slice());
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut data = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
        data.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => data,
            png::ColorType::Rgb => data
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            c => return Err(format!("扱えない PNG の色形式: {c:?}")),
        };
        Ok(Image {
            width: info.width,
            height: info.height,
            rgba,
        })
    }
}

// ponytail: macOS（WKWebView の takeSnapshot）は未実装。
#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use tauri::webview::PlatformWebview;

    use super::Done;

    pub fn snapshot(_: PlatformWebview, done: Done) {
        done(Err("このプラットフォームでは WebView を読み戻せない".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb32_is_unpremultiplied_to_straight_rgba() {
        let px = |v: u32| v.to_ne_bytes();
        // 2x1 の画像、行末に 4 バイトの詰め物。
        let mut data = Vec::new();
        data.extend(px(0xff_10_20_30)); // 不透明
        data.extend(px(0x80_40_00_80)); // 半透明（乗算済み）
        data.extend([9, 9, 9, 9]);
        assert_eq!(
            from_argb32(&data, 2, 1, 12, true),
            [0x10, 0x20, 0x30, 0xff, 0x80, 0x00, 0xff, 0x80]
        );
        // 透明は黒に、RGB24 はアルファを無視して不透明に。
        assert_eq!(from_argb32(&px(0x00_00_00_00), 1, 1, 4, true), [0, 0, 0, 0]);
        assert_eq!(
            from_argb32(&px(0x00_01_02_03), 1, 1, 4, false),
            [1, 2, 3, 255]
        );
    }
}
