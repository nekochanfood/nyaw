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
            .map_err(|s| format!("not an image surface: {:?}", s.type_()))?;
        img.flush();
        let (width, height) = (img.width() as u32, img.height() as u32);
        let stride = img.stride() as usize;
        let alpha = match img.format() {
            cairo::Format::ARgb32 => true,
            cairo::Format::Rgb24 => false,
            f => return Err(format!("unsupported pixel format: {f:?}")),
        };
        let data = img.data().map_err(|e| e.to_string())?;
        Ok(Image {
            width,
            height,
            rgba: from_argb32(&data, width, height, stride, alpha),
        })
    }
}

// Windows は画面外に置いた WebView2 の CapturePreview で読み戻す。
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
                let bytes = r.map_err(|e| e.to_string()).and_then(|_| read(&png));
                // PNG の展開は重い（800x600 で数十 ms）。メインスレッド（フレームループ）を塞がないよう別スレッドで行う。
                let finish = finish.clone();
                std::thread::spawn(move || finish(bytes.and_then(|b| decode(&b))));
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

    /// CapturePreview が書いた PNG のバイト列を読み出す。
    fn read(stream: &IStream) -> Result<Vec<u8>, String> {
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
        Ok(bytes)
    }

    /// PNG を RGBA8 にする。
    fn decode(bytes: &[u8]) -> Result<Image, String> {
        let mut decoder = png::Decoder::new(bytes);
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
            c => return Err(format!("unsupported PNG color format: {c:?}")),
        };
        Ok(Image {
            width: info.width,
            height: info.height,
            rgba,
        })
    }
}

/// 乗算済みアルファの RGBA8 をストレートアルファにする。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn from_rgba_premultiplied(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.as_chunks_mut::<4>().0 {
        match px[3] {
            0 => *px = [0; 4],
            255 => {}
            a => {
                for c in &mut px[..3] {
                    *c = unpremultiply(*c, a);
                }
            }
        }
    }
    rgba
}

// macOS は WKWebView の takeSnapshot で読み戻す（非表示のウィンドウでも撮れる）。
#[cfg(target_os = "macos")]
mod platform {
    use std::sync::Mutex;

    use block2::RcBlock;
    use objc2_app_kit::NSImage;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    };
    use objc2_foundation::NSError;
    use objc2_web_kit::WKWebView;
    use tauri::webview::PlatformWebview;

    use super::{Done, Image, from_rgba_premultiplied};

    pub fn snapshot(webview: PlatformWebview, done: Done) {
        // SAFETY: wry の WKWebView。with_webview の中（メインスレッド）で使い、ブロックも呼ばれるまで生きている。
        let view: &WKWebView = unsafe { &*webview.inner().cast() };
        // ブロックは Fn なので、1 回だけ呼べるよう取り出せる形で持つ。
        let done = Mutex::new(Some(done));
        let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let Some(done) = done.lock().ok().and_then(|mut d| d.take()) else {
                return;
            };
            // SAFETY: WebKit が渡すのは null か有効な NSImage / NSError。
            done(match unsafe { (image.as_ref(), error.as_ref()) } {
                (Some(image), _) => read(image),
                (None, Some(e)) => Err(e.localizedDescription().to_string()),
                (None, None) => Err("snapshot failed".into()),
            });
        });
        unsafe { view.takeSnapshotWithConfiguration_completionHandler(None, &block) };
    }

    /// NSImage を RGBA8 のビットマップに描いて読み出す。
    fn read(image: &NSImage) -> Result<Image, String> {
        // SAFETY: 矩形・文脈・ヒントは省く（そのままの大きさで取り出す）。
        let cg =
            unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }
                .ok_or("cannot convert to CGImage")?;
        let (w, h) = (CGImage::width(Some(&cg)), CGImage::height(Some(&cg)));
        let mut rgba = vec![0u8; w * h * 4];
        let space = CGColorSpace::new_device_rgb();
        // SAFETY: rgba は w*h*4 バイトで、文脈より長く生きる。
        let ctx = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                w,
                h,
                8,
                w * 4,
                space.as_deref(),
                CGImageAlphaInfo::PremultipliedLast.0,
            )
        }
        .ok_or("cannot create bitmap context")?;
        let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
        CGContext::draw_image(Some(&ctx), rect, Some(&cg));
        drop(ctx);
        Ok(Image {
            width: w as u32,
            height: h as u32,
            rgba: from_rgba_premultiplied(rgba),
        })
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod platform {
    use tauri::webview::PlatformWebview;

    use super::Done;

    pub fn snapshot(_: PlatformWebview, done: Done) {
        done(Err("cannot read back WebView on this platform".into()))
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

    #[test]
    fn premultiplied_rgba_is_unpremultiplied() {
        assert_eq!(
            from_rgba_premultiplied(vec![
                0x10, 0x20, 0x30, 0xff, 0x20, 0x00, 0x40, 0x80, 9, 9, 9, 0
            ]),
            [0x10, 0x20, 0x30, 0xff, 0x40, 0x00, 0x80, 0x80, 0, 0, 0, 0]
        );
    }
}
