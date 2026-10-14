//! Windows: WebView のウィンドウを Windows.Graphics.Capture で撮り続ける。中身が変わるたびに DWM から GPU のテクスチャで
//! フレームが届き（画面の更新に合わせて最大でリフレッシュレートまで）、スレッドプールの上で読み出して `sink` に渡す。
//! CapturePreview（PNG を作らせて展開する。1 枚に数十 ms、同時に 1 枚）よりずっと速く、メインスレッドも使わない。
//! WGC は非表示のウィンドウを撮れないので、パネルのウィンドウは表示したまま画面の外に置く。

use std::sync::{Arc, Mutex};

use tauri::{WebviewWindow, Wry};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND, POINT, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::Interface;

use super::capture::Image;

/// 届いた絵（クライアント領域だけ、RGBA8 のストレートアルファ）を受け取る。スレッドプールから呼ばれる。
pub type Sink = Arc<dyn Fn(Result<Image, String>) + Send + Sync>;

const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;
/// フレームプールのバッファ数。
const BUFFERS: i32 = 2;

/// 1 枚のウィンドウを撮り続ける。落とすと止まる。
pub struct Stream {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: i64,
}

/// スレッドプールの上で使う、読み出しの状態。
struct Reader {
    hwnd: isize,
    device: IDirect3DDevice,
    d3d: ID3D11Device,
    context: ID3D11DeviceContext,
    /// フレームプールの大きさ（ウィンドウが変わったら作り直す）。
    size: SizeInt32,
    /// CPU で読むためのテクスチャ（大きさが変わったら作り直す）。
    staging: Option<(ID3D11Texture2D, u32, u32)>,
}

// SAFETY: IDirect3DDevice（WinRT の Direct3D デバイス）は agile なオブジェクトで、どのスレッドから使ってもよい。
// ほかは windows クレートが Send にしている。Reader は Mutex の中で、フレームプールのハンドラから 1 本ずつだけ使う。
unsafe impl Send for Reader {}

fn err(what: &str) -> impl FnOnce(windows::core::Error) -> String + '_ {
    move |e| format!("{what}: {e}")
}

impl Stream {
    /// `window` を撮り始める。中身が変わるたびに `sink` に絵が届く。
    pub fn start(window: &WebviewWindow<Wry>, sink: Sink) -> Result<Self, String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
        // SAFETY: Win32 / WinRT の呼び出し。引数はここで作った有効な値。
        unsafe {
            let (mut d3d, mut context) = (None, None);
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                Some(&mut context),
            )
            .map_err(err("D3D11 のデバイスを作れない"))?;
            let (d3d, context): (ID3D11Device, ID3D11DeviceContext) = (
                d3d.ok_or("D3D11 のデバイスが無い")?,
                context.ok_or("D3D11 のコンテキストが無い")?,
            );
            let dxgi: IDXGIDevice = d3d.cast().map_err(err("DXGI のデバイス"))?;
            let device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
                .and_then(|d| d.cast())
                .map_err(err("WinRT の Direct3D デバイス"))?;
            let interop =
                windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                    .map_err(err("GraphicsCaptureItem"))?;
            let item: GraphicsCaptureItem = interop
                .CreateForWindow(HWND(hwnd as _))
                .map_err(err("ウィンドウを撮る対象にできない"))?;
            let size = item.Size().map_err(err("撮る大きさ"))?;
            let pool =
                Direct3D11CaptureFramePool::CreateFreeThreaded(&device, FORMAT, BUFFERS, size)
                    .map_err(err("フレームプール"))?;
            let session = pool
                .CreateCaptureSession(&item)
                .map_err(err("撮影のセッション"))?;
            // 黄色い枠とマウスカーソルは要らない（枠を消せない古い Windows では無視する）。
            let _ = session.SetIsBorderRequired(false);
            let _ = session.SetIsCursorCaptureEnabled(false);
            let reader = Mutex::new(Reader {
                hwnd,
                device,
                d3d,
                context,
                size,
                staging: None,
            });
            let handler = TypedEventHandler::<
                Direct3D11CaptureFramePool,
                windows::core::IInspectable,
            >::new(move |pool, _| {
                let Ok(pool) = pool.ok() else { return Ok(()) };
                let Ok(mut r) = reader.lock() else {
                    return Ok(());
                };
                match r.read(pool) {
                    Ok(Some(img)) => sink(Ok(img)),
                    Ok(None) => {}
                    Err(e) => sink(Err(e)),
                }
                Ok(())
            });
            let token = pool.FrameArrived(&handler).map_err(err("フレームの購読"))?;
            session.StartCapture().map_err(err("撮影を始められない"))?;
            Ok(Self {
                pool,
                session,
                token,
            })
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

impl Reader {
    /// 届いた最新のフレームを読み、クライアント領域を切り出してストレートアルファの RGBA8 にする。
    fn read(&mut self, pool: &Direct3D11CaptureFramePool) -> Result<Option<Image>, String> {
        let Ok(frame) = pool.TryGetNextFrame() else {
            return Ok(None);
        };
        let content = frame.ContentSize().map_err(err("フレームの大きさ"))?;
        // SAFETY: このリーダーのデバイスとコンテキストだけを使う（フレームプールのハンドラは 1 本ずつ呼ばれる）。
        unsafe {
            let access: IDirect3DDxgiInterfaceAccess = frame
                .Surface()
                .and_then(|s| s.cast())
                .map_err(err("フレームのサーフェス"))?;
            let texture: ID3D11Texture2D =
                access.GetInterface().map_err(err("フレームのテクスチャ"))?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            let (fw, fh) = (desc.Width, desc.Height);
            if self.staging.as_ref().is_none_or(|s| (s.1, s.2) != (fw, fh)) {
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                    ..desc
                };
                let mut t = None;
                self.d3d
                    .CreateTexture2D(&staging_desc, None, Some(&mut t))
                    .map_err(err("読み出し用のテクスチャ"))?;
                self.staging = Some((t.ok_or("読み出し用のテクスチャが無い")?, fw, fh));
            }
            let staging = &self.staging.as_ref().unwrap().0;
            self.context.CopyResource(staging, &texture);
            drop(frame);
            // ウィンドウの大きさが変わったら、次のフレームから新しい大きさで撮る。
            if content != self.size {
                self.size = content;
                pool.Recreate(&self.device, FORMAT, BUFFERS, content)
                    .map_err(err("フレームプールを作り直せない"))?;
            }
            // 撮れるのはウィンドウ全体（枠を含む）。クライアント領域の位置と大きさで切り出す。
            let (ox, oy, cw, ch) = self.client_in_frame()?;
            let (cw, ch) = (
                cw.min((content.Width as u32).min(fw).saturating_sub(ox)),
                ch.min((content.Height as u32).min(fh).saturating_sub(oy)),
            );
            if cw <= 1 || ch <= 1 {
                return Ok(None);
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(err("読み出せない"))?;
            let pitch = mapped.RowPitch as usize;
            let src = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * fh as usize);
            let mut rgba = Vec::with_capacity((cw * ch * 4) as usize);
            for y in oy..oy + ch {
                let row = &src[y as usize * pitch + ox as usize * 4..][..cw as usize * 4];
                for px in row.as_chunks::<4>().0 {
                    let [b, g, r, a] = *px;
                    rgba.extend_from_slice(&straight(r, g, b, a));
                }
            }
            self.context.Unmap(staging, 0);
            Ok(Some(Image {
                width: cw,
                height: ch,
                rgba,
            }))
        }
    }

    /// 撮った絵（ウィンドウの見た目の範囲）の中での、クライアント領域の左上と大きさ（px）。
    fn client_in_frame(&self) -> Result<(u32, u32, u32, u32), String> {
        let hwnd = HWND(self.hwnd as _);
        // SAFETY: 読むだけの Win32 の呼び出し。
        unsafe {
            let mut frame = RECT::default();
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut frame as *mut RECT as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
            .map_err(err("ウィンドウの範囲"))?;
            let mut client = RECT::default();
            GetClientRect(hwnd, &mut client).map_err(err("クライアント領域"))?;
            let mut origin = POINT::default();
            let _ = ClientToScreen(hwnd, &mut origin);
            Ok((
                (origin.x - frame.left).max(0) as u32,
                (origin.y - frame.top).max(0) as u32,
                (client.right - client.left).max(0) as u32,
                (client.bottom - client.top).max(0) as u32,
            ))
        }
    }
}

/// DWM の BGRA は乗算済みアルファ。ストレートアルファの RGBA にする。
fn straight(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    match a {
        0 => [0, 0, 0, 0],
        255 => [r, g, b, 255],
        a => {
            let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            [un(r), un(g), un(b), a]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_bgra_becomes_straight_rgba() {
        assert_eq!(straight(10, 20, 30, 255), [10, 20, 30, 255]);
        assert_eq!(straight(0, 0, 0, 0), [0, 0, 0, 0]);
        // 50% の白（乗算済みで 128）は、ストレートでは白のまま半透明。
        assert_eq!(straight(128, 128, 128, 128), [255, 255, 255, 128]);
    }
}
