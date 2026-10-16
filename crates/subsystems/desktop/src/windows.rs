use super::{Error, Frame, Session, Source, SourceKind, Touch, TouchPhase};
use ::windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Foundation::{E_POINTER, ERROR_NOT_READY, HMODULE, HWND, LPARAM, POINT, RECT},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::*,
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Dxgi::IDXGIDevice,
            Gdi::*,
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
        },
        UI::{
            Controls::*,
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
            Input::Pointer::*,
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, Interface, factory},
};
use std::{
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

impl From<::windows::core::Error> for Error {
    fn from(e: ::windows::core::Error) -> Self {
        Self(e.to_string())
    }
}

pub(super) fn sources() -> Result<Vec<Source>, Error> {
    let mut sources = Vec::new();
    unsafe extern "system" fn monitor(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let sources = unsafe { &mut *(data.0 as *mut Vec<Source>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) }.as_bool() {
            let len = info
                .szDevice
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(info.szDevice.len());
            sources.push(Source {
                name: String::from_utf16_lossy(&info.szDevice[..len]),
                kind: SourceKind::Screen,
                target: monitor.0 as isize,
            });
        }
        true.into()
    }
    unsafe extern "system" fn window(hwnd: HWND, data: LPARAM) -> BOOL {
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return true.into();
        }
        let mut cloaked = 0u32;
        let _ = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as _,
                size_of::<u32>() as u32,
            )
        };
        if cloaked != 0 {
            return true.into();
        }
        let mut title = [0u16; 1024];
        let len = unsafe { GetWindowTextW(hwnd, &mut title) };
        if len > 0 {
            unsafe { &mut *(data.0 as *mut Vec<Source>) }.push(Source {
                name: String::from_utf16_lossy(&title[..len as usize]),
                kind: SourceKind::Window,
                target: hwnd.0 as isize,
            });
        }
        true.into()
    }
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor),
            LPARAM(&mut sources as *mut _ as isize),
        )
        .ok()?;
        EnumWindows(Some(window), LPARAM(&mut sources as *mut _ as isize))?;
    }
    Ok(sources)
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

pub(super) struct Capture {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    _item: GraphicsCaptureItem,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    direct: IDirect3DDevice,
    size: SizeInt32,
    staging: Option<ID3D11Texture2D>,
    ready: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    source: Source,
    touch: Option<TouchDevice>,
    _apartment: Apartment,
}

impl Capture {
    pub fn new(source: Source) -> Result<Self, Error> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        let apartment = Apartment;
        // WGC と入力の双方をデバイスピクセルで扱い、異なる DPI のモニター間でも座標を揃える。
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(Error("Windows Graphics Capture is unavailable".into()));
        }
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe {
            match source.kind {
                SourceKind::Window => interop.CreateForWindow(HWND(source.target as _))?,
                SourceKind::Screen => interop.CreateForMonitor(HMONITOR(source.target as _))?,
            }
        };
        let size = item.Size()?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(Error("capture source has no pixels".into()));
        }
        let (mut device, mut context) = (None, None);
        let create = |driver, device: &mut _, context: &mut _| unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(device),
                None,
                Some(context),
            )
        };
        create(D3D_DRIVER_TYPE_HARDWARE, &mut device, &mut context)
            .or_else(|_| create(D3D_DRIVER_TYPE_WARP, &mut device, &mut context))?;
        let device: ID3D11Device = device.ok_or_else(|| Error("D3D11 device missing".into()))?;
        let context = context.ok_or_else(|| Error("D3D11 context missing".into()))?;
        let dxgi: IDXGIDevice = device.cast()?;
        let direct: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &direct,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let ready = Arc::new(AtomicBool::new(false));
        let signal = ready.clone();
        pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
            signal.store(true, Ordering::Release);
            Ok(())
        }))?;
        let closed = Arc::new(AtomicBool::new(false));
        let signal = closed.clone();
        item.Closed(&TypedEventHandler::new(move |_, _| {
            signal.store(true, Ordering::Release);
            Ok(())
        }))?;
        let session = pool.CreateCaptureSession(&item)?;
        session.StartCapture()?;
        Ok(Self {
            session,
            pool,
            _item: item,
            device,
            context,
            direct,
            size,
            staging: None,
            ready,
            closed,
            source,
            touch: None,
            _apartment: apartment,
        })
    }

    fn bounds(&self) -> Result<RECT, Error> {
        let mut rect = RECT::default();
        unsafe {
            match self.source.kind {
                SourceKind::Window => {
                    let hwnd = HWND(self.source.target as _);
                    if IsIconic(hwnd).as_bool() || !IsWindow(Some(hwnd)).as_bool() {
                        return Err(Error("capture window is minimized or closed".into()));
                    }
                    DwmGetWindowAttribute(
                        hwnd,
                        DWMWA_EXTENDED_FRAME_BOUNDS,
                        &mut rect as *mut _ as _,
                        size_of::<RECT>() as u32,
                    )?;
                }
                SourceKind::Screen => {
                    let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    GetMonitorInfoW(HMONITOR(self.source.target as _), &mut info).ok()?;
                    rect = info.rcMonitor;
                }
            }
        }
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return Err(Error("capture source has invalid bounds".into()));
        }
        Ok(rect)
    }
}

impl Session for Capture {
    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        if !self.ready.swap(false, Ordering::AcqRel) {
            return Ok(None);
        }
        let frame = match self.pool.TryGetNextFrame() {
            Ok(frame) => frame,
            Err(e) if e.code() == E_POINTER => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let result = (|| {
            let size = frame.ContentSize()?;
            if size.Width <= 0 || size.Height <= 0 {
                return Ok(None);
            }
            if size != self.size {
                self.size = size;
                self.staging = None;
                return Ok(None);
            }
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                texture.GetDesc(&mut desc);
            }
            let (width, height) = (size.Width as u32, size.Height as u32);
            if width > desc.Width || height > desc.Height {
                return Ok(None);
            }
            if let Some(staging) = &self.staging {
                let mut previous = D3D11_TEXTURE2D_DESC::default();
                unsafe {
                    staging.GetDesc(&mut previous);
                }
                if previous.Width != desc.Width || previous.Height != desc.Height {
                    self.staging = None;
                }
            }
            if self.staging.is_none() {
                desc.Usage = D3D11_USAGE_STAGING;
                desc.BindFlags = 0;
                desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                desc.MiscFlags = 0;
                unsafe {
                    self.device
                        .CreateTexture2D(&desc, None, Some(&mut self.staging))?;
                }
            }
            let staging = self
                .staging
                .as_ref()
                .ok_or_else(|| Error("D3D11 staging texture missing".into()))?;
            unsafe {
                self.context.CopyResource(staging, &texture);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                self.context
                    .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                let bytes = (mapped.RowPitch as usize).checked_mul(height as usize);
                let result = if mapped.pData.is_null()
                    || bytes.is_none_or(|n| n > 256 * 1024 * 1024 || n > isize::MAX as usize)
                {
                    Err(Error("invalid mapped desktop texture".into()))
                } else {
                    Frame::from_bgra(
                        width,
                        height,
                        mapped.RowPitch as usize,
                        std::slice::from_raw_parts(mapped.pData as *const u8, bytes.unwrap()),
                    )
                };
                self.context.Unmap(staging, 0);
                result.map(Some)
            }
        })();
        frame.Close()?;
        // リサイズ途中の画像は捨て、フレームの参照を解放してからプールを作り直す。
        if self.staging.is_none() {
            self.pool.Recreate(
                &self.direct,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                self.size,
            )?;
        }
        result
    }

    fn touch(&mut self, touch: Touch) -> Result<(), Error> {
        let bounds = self.bounds()?;
        let point = normalized_point(bounds, touch.x, touch.y);
        if touch.phase == TouchPhase::Down && self.source.kind == SourceKind::Window {
            let hwnd = HWND(self.source.target as _);
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
            // タッチはデスクトップへ届くため、選んだ窓が隠れたまま別の窓を操作しないよう確認する。
            let hit = unsafe { WindowFromPoint(point) };
            if unsafe { GetAncestor(hit, GA_ROOT) } != hwnd {
                return Err(Error(
                    "capture window must be visible at the touch point".into(),
                ));
            }
        }
        if self.touch.is_none() {
            self.touch = Some(TouchDevice::new()?);
        }
        self.touch
            .as_mut()
            .unwrap()
            .send(touch.id, touch.phase, point)
    }

    fn heartbeat(&mut self) -> Result<(), Error> {
        if let Some(touch) = &mut self.touch {
            touch.heartbeat()?;
        }
        Ok(())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.touch.take();
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

fn normalized_point(bounds: RECT, x: f32, y: f32) -> POINT {
    POINT {
        x: bounds.left + ((bounds.right - bounds.left - 1) as f32 * x).round() as i32,
        y: bounds.top + ((bounds.bottom - bounds.top - 1) as f32 * y).round() as i32,
    }
}

struct TouchDevice {
    device: HSYNTHETICPOINTERDEVICE,
    contacts: [Option<POINT>; 2],
}
impl TouchDevice {
    fn new() -> Result<Self, Error> {
        Ok(Self {
            device: unsafe { CreateSyntheticPointerDevice(PT_TOUCH, 2, POINTER_FEEDBACK_NONE)? },
            contacts: [None; 2],
        })
    }

    fn inject(&self, changed: Option<(usize, POINT, POINTER_FLAGS)>) -> Result<(), Error> {
        let mut pointers = Vec::with_capacity(2);
        for id in 0..2 {
            let contact = changed
                .filter(|c| c.0 == id)
                .map(|(_, p, f)| (p, f))
                .or_else(|| {
                    self.contacts[id].map(|p| {
                        (
                            p,
                            POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                        )
                    })
                });
            if let Some((point, flags)) = contact {
                pointers.push(POINTER_TYPE_INFO {
                    r#type: PT_TOUCH,
                    Anonymous: POINTER_TYPE_INFO_0 {
                        touchInfo: POINTER_TOUCH_INFO {
                            pointerInfo: POINTER_INFO {
                                pointerType: PT_TOUCH,
                                pointerId: id as u32 + 1,
                                pointerFlags: flags,
                                ptPixelLocation: point,
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    },
                });
            }
        }
        if !pointers.is_empty() {
            for attempt in 0..4 {
                match unsafe { InjectSyntheticPointerInput(self.device, &pointers) } {
                    Err(e)
                        if e.code() == ::windows::core::HRESULT::from_win32(ERROR_NOT_READY.0)
                            && attempt < 3 =>
                    {
                        // 同じ時刻の接点を Windows が拒む場合だけ、ワーカーで次の時刻まで待つ。
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    result => return result.map_err(Into::into),
                }
            }
        }
        Ok(())
    }

    fn send(&mut self, id: u32, phase: TouchPhase, point: POINT) -> Result<(), Error> {
        let id = id as usize;
        let active = self.contacts[id].is_some();
        if phase == TouchPhase::Down && active {
            return Err(Error("duplicate touch down".into()));
        }
        if phase != TouchPhase::Down && !active {
            return Ok(());
        }
        if phase == TouchPhase::Up && self.contacts[id] != Some(point) {
            self.send(id as u32, TouchPhase::Move, point)?;
        }
        let flags = match phase {
            TouchPhase::Down => POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
            TouchPhase::Move => POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
            TouchPhase::Up => POINTER_FLAG_UP,
            TouchPhase::Cancel => POINTER_FLAG_UP | POINTER_FLAG_CANCELED,
        };
        let point = if phase == TouchPhase::Cancel {
            self.contacts[id].unwrap()
        } else {
            point
        };
        self.inject(Some((id, point, flags)))?;
        self.contacts[id] = if matches!(phase, TouchPhase::Up | TouchPhase::Cancel) {
            None
        } else {
            Some(point)
        };
        Ok(())
    }

    fn heartbeat(&self) -> Result<(), Error> {
        self.inject(None)
    }
}
impl Drop for TouchDevice {
    fn drop(&mut self) {
        for id in 0..2 {
            if let Some(point) = self.contacts[id] {
                let _ = self.send(id as u32, TouchPhase::Cancel, point);
            }
        }
        unsafe {
            DestroySyntheticPointerDevice(self.device);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_negative_monitor_origin_and_clamps_to_last_pixel() {
        let bounds = RECT {
            left: -1920,
            top: -100,
            right: 0,
            bottom: 980,
        };
        assert_eq!(
            normalized_point(bounds, 0.0, 0.0),
            POINT { x: -1920, y: -100 }
        );
        assert_eq!(normalized_point(bounds, 1.0, 1.0), POINT { x: -1, y: 979 });
    }
}
