use super::{Error, Frame, Session, Source, SourceKind, Touch, TouchPhase, Wake};
use ::windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Foundation::{ERROR_NOT_READY, HMODULE, HWND, LPARAM, POINT, RECT},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::*,
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Dxgi::IDXGIDevice,
            Gdi::*,
        },
        System::{
            Com::CoIncrementMTAUsage,
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Threading::{AttachThreadInput, GetCurrentThreadId},
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
                RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
            },
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
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
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
    pub fn new(source: Source, wake: Wake) -> Result<Self, Error> {
        // WinRT の静的キャッシュと MTA の寿命を揃え、全配信停止後の再開でも参照を有効に保つ。
        static MTA: OnceLock<Result<(), Error>> = OnceLock::new();
        MTA.get_or_init(|| {
            unsafe { CoIncrementMTAUsage() }
                .map(|_| ())
                .map_err(Into::into)
        })
        .clone()?;
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        let apartment = Apartment;
        // WGC と入力の双方をデバイスピクセルで扱い、異なる DPI のモニター間でも座標を揃える。
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        if source.kind == SourceKind::Window
            && unsafe { IsIconic(HWND(source.target as _)) }.as_bool()
        {
            // 最小化した窓は WGC が画像を出さず、最初の一枚を待ったまま止まるので元に戻す。
            unsafe {
                let _ = ShowWindowAsync(HWND(source.target as _), SW_RESTORE);
            }
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
            wake.wake();
            Ok(())
        }))?;
        let closed = Arc::new(AtomicBool::new(false));
        let signal = closed.clone();
        item.Closed(&TypedEventHandler::new(move |_, _| {
            signal.store(true, Ordering::Release);
            Ok(())
        }))?;
        let session = pool.CreateCaptureSession(&item)?;
        // 既定では OS がリフレッシュレートより粗く間引く。毎 vsync で出させ、上限 FPS はワーカーで揃える
        // （上限の間隔を渡すと vsync の倍数へ切り上げられ、143Hz で 60 を頼むと 48 になる）。
        // Windows 11 24H2 より前には無い設定なので、失敗しても既定のまま続ける。
        let _ = session.SetMinUpdateInterval(TimeSpan { Duration: 10_000 });
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

    fn activate_touch_target(&mut self, touch: Touch) -> Result<Option<POINT>, Error> {
        let hwnd = HWND(self.source.target as _);
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            }
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
            if !SetForegroundWindow(hwnd).as_bool() {
                // VR の入力だけでは前面化の権限がない場合がある。切り替え中だけ入力キューを共有する。
                let current = GetCurrentThreadId();
                let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
                let mut message = MSG::default();
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                if foreground != 0
                    && foreground != current
                    && AttachThreadInput(current, foreground, true).as_bool()
                {
                    let _ = SetForegroundWindow(hwnd);
                    let _ = AttachThreadInput(current, foreground, false);
                }
            }
        }
        // 前面化は相手の UI スレッドで反映されるため、直後の一回の判定では拒否しない。
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
        loop {
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                return Ok(None);
            }
            if let Ok(bounds) = self.bounds() {
                let point = normalized_point(bounds, touch.x, touch.y);
                if unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) } == hwnd {
                    return Ok(Some(point));
                }
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            self.heartbeat()?;
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
    }

    fn next_frame(
        &self,
    ) -> Result<Option<::windows::Graphics::Capture::Direct3D11CaptureFrame>, Error> {
        // WinRT の正常な null 戻り値を「フレームなし」として扱う。非 nullable な Rust API は Error(0) に変換する。
        unsafe {
            let mut frame = std::ptr::null_mut();
            (Interface::vtable(&self.pool).TryGetNextFrame)(
                Interface::as_raw(&self.pool),
                &mut frame,
            )
            .ok()?;
            Ok((!frame.is_null())
                .then(|| ::windows::Graphics::Capture::Direct3D11CaptureFrame::from_raw(frame)))
        }
    }
}

impl Session for Capture {
    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            || (self.source.kind == SourceKind::Window
                && !unsafe { IsWindow(Some(HWND(self.source.target as _))) }.as_bool())
    }

    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        if !self.ready.swap(false, Ordering::AcqRel) {
            return Ok(None);
        }
        let Some(mut frame) = self.next_frame()? else {
            return Ok(None);
        };
        // プールは二枚なので、低 FPS で溜まったもう一枚があれば最新の方だけコピーする。
        match self.next_frame() {
            Ok(Some(newer)) => {
                frame.Close()?;
                frame = newer;
            }
            Ok(None) => (),
            Err(e) => {
                let _ = frame.Close();
                return Err(e);
            }
        }
        let captured = frame.SystemRelativeTime().map(captured_at);
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
        drop(frame);
        let result = result.map(|f| {
            f.map(|f| Frame {
                captured: captured.unwrap_or(f.captured),
                ..f
            })
        });
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
        if touch.phase != TouchPhase::Down
            && !self
                .touch
                .as_ref()
                .is_some_and(|device| device.contacts[touch.id as usize].is_some())
        {
            return Ok(());
        }
        let point = if touch.phase == TouchPhase::Down && self.source.kind == SourceKind::Window {
            let Some(point) = self.activate_touch_target(touch)? else {
                // 他の窓への誤入力を避ける。前面化できないクリックだけを捨て、画像配信は続ける。
                return Ok(());
            };
            point
        } else {
            normalized_point(self.bounds()?, touch.x, touch.y)
        };
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

/// WGC のフレーム時刻（QPC を 100ns 単位にしたもの）を `Instant` にする。
fn captured_at(time: TimeSpan) -> Instant {
    let (mut now, mut frequency) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut now);
        let _ = QueryPerformanceFrequency(&mut frequency);
    }
    let now = (now as i128 * 10_000_000 / frequency.max(1) as i128) as i64;
    let age = Duration::from_nanos(now.saturating_sub(time.Duration).max(0) as u64 * 100);
    Instant::now().checked_sub(age).unwrap_or_else(Instant::now)
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
    injected: Instant,
}
impl TouchDevice {
    fn new() -> Result<Self, Error> {
        Ok(Self {
            device: unsafe { CreateSyntheticPointerDevice(PT_TOUCH, 2, POINTER_FEEDBACK_NONE)? },
            contacts: [None; 2],
            injected: Instant::now(),
        })
    }

    fn inject(&mut self, changed: Option<(usize, POINT, POINTER_FLAGS)>) -> Result<(), Error> {
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
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    result => {
                        self.injected = Instant::now();
                        return result.map_err(Into::into);
                    }
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

    /// 押したままの接点を維持する。移動の直後に重ねると同じ時刻として拒まれるので、注入が途絶えた時だけ送る。
    fn heartbeat(&mut self) -> Result<(), Error> {
        if self.injected.elapsed() < Duration::from_millis(8) {
            return Ok(());
        }
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
