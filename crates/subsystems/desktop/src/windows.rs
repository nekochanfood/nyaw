use super::{
    Button, Error, Frame, Icon, InstalledApp, Session, Source, SourceKind, Touch, TouchPhase, Wake,
};
use ::windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
            DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
            DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
            QueryDisplayConfig,
        },
        Foundation::{CloseHandle, HMODULE, HWND, LPARAM, POINT, RECT, SIZE, WPARAM},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::*,
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Dxgi::IDXGIDevice,
            Gdi::*,
        },
        Storage::EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_Link_TargetParsingPath},
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoIncrementMTAUsage, CoInitializeEx, CoTaskMemFree},
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Threading::{
                AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
                RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
            },
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
            Input::KeyboardAndMouse::*,
            Shell::{
                BHID_EnumItems, ExtractIconExW, FOLDERID_AppsFolder, IEnumShellItems, IShellItem,
                IShellItem2, IShellItemImageFactory, KF_FLAG_DEFAULT, SHGetKnownFolderItem,
                SIGDN_NORMALDISPLAY, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY, ShellExecuteW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, Interface, PCWSTR, PWSTR, factory},
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
    let mut sources: Vec<Source> = Vec::new();
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
                icon: None,
                app: None,
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
                icon: unsafe { window_icon(hwnd) },
                app: unsafe { process_path(hwnd) }.and_then(|p| {
                    let p = String::from_utf16_lossy(&p);
                    p.rsplit('\\').next().map(str::to_owned)
                }),
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
    // 画面の名前は "\\.\DISPLAY2" のような装置名なので、モニターの製品名（無ければ「Display 2」）にする。
    let products = monitor_products();
    let screens: Vec<(String, Option<String>)> = sources
        .iter()
        .filter(|s| s.kind == SourceKind::Screen)
        .map(|s| (s.name.clone(), products.get(&s.name).cloned()))
        .collect();
    for source in sources.iter_mut().filter(|s| s.kind == SourceKind::Screen) {
        source.name = screen_label(&source.name, &screens);
    }
    Ok(sources)
}

/// GDI の装置名（"\\.\DISPLAY2"）→ そこに映しているモニターの製品名（EDID の名前。"DELL U2720Q" など）。
fn monitor_products() -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    let (mut paths, mut modes) = (0u32, 0u32);
    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) }
        .is_err()
    {
        return names;
    }
    let mut path_info = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
    let mut mode_info = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
    let queried = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut paths,
            path_info.as_mut_ptr(),
            &mut modes,
            mode_info.as_mut_ptr(),
            None,
        )
    };
    if queried.is_err() {
        return names;
    }
    for path in &path_info[..paths as usize] {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..Default::default()
        };
        let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0
            || unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0
        {
            continue;
        }
        let text = |w: &[u16]| {
            String::from_utf16_lossy(&w[..w.iter().position(|c| *c == 0).unwrap_or(w.len())])
        };
        let product = text(&target.monitorFriendlyDeviceName);
        if !product.trim().is_empty() {
            names.insert(text(&source.viewGdiDeviceName), product.trim().to_owned());
        }
    }
    names
}

/// 画面の表示名。モニターの製品名（同じ製品名がほかにもあれば番号を添える）か、無ければ「Display 番号」。
fn screen_label(device: &str, screens: &[(String, Option<String>)]) -> String {
    let number = device
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .to_owned();
    let product = screens
        .iter()
        .find(|(d, _)| d == device)
        .and_then(|(_, p)| p.clone());
    match product {
        Some(p)
            if screens
                .iter()
                .filter(|(_, q)| q.as_ref() == Some(&p))
                .count()
                > 1 =>
        {
            format!("{p} ({number})")
        }
        Some(p) => p,
        None if number.is_empty() => device.to_owned(),
        None => format!("Display {number}"),
    }
}

/// ウィンドウのアイコン。ウィンドウ自身のもの（WM_GETICON・クラス）が無ければ、プロセスの実行ファイルのもの。
unsafe fn window_icon(hwnd: HWND) -> Option<Icon> {
    let mut icon = 0usize;
    // 固まったウィンドウで一覧の取得ごと止まらないよう、待ちを区切る。
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            100,
            Some(&mut icon),
        )
    };
    if sent.0 == 0 || icon == 0 {
        icon = unsafe { GetClassLongPtrW(hwnd, GCLP_HICON) };
    }
    if icon != 0 {
        // 借りたアイコンなので破棄しない。
        return unsafe { icon_rgba(HICON(icon as _)) };
    }
    let mut path = unsafe { process_path(hwnd) }?;
    path.push(0);
    let mut large = HICON::default();
    unsafe { ExtractIconExW(PCWSTR(path.as_ptr()), 0, Some(&mut large), None, 1) };
    if large.is_invalid() {
        return None;
    }
    let rgba = unsafe { icon_rgba(large) };
    let _ = unsafe { DestroyIcon(large) };
    rgba
}

/// ウィンドウのプロセスの実行ファイルのパス（UTF-16、終端なし）。
unsafe fn process_path(hwnd: HWND) -> Option<Vec<u16>> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut path = [0u16; 1024];
    let mut len = path.len() as u32;
    let named = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    named.ok()?;
    Some(path[..len as usize].to_vec())
}

/// アイコンを RGBA で読み出す。アルファを持たない古いアイコンはマスクから透明部分を作る。
unsafe fn icon_rgba(icon: HICON) -> Option<Icon> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let read = |bitmap: HBITMAP| unsafe { bitmap_bgra(bitmap) };
    let color = (!info.hbmColor.is_invalid())
        .then(|| read(info.hbmColor))
        .flatten();
    let mask = color
        .as_ref()
        .filter(|(_, _, px)| px.as_chunks::<4>().0.iter().all(|p| p[3] == 0))
        .and_then(|_| read(info.hbmMask));
    unsafe {
        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
    }
    let (width, height, mut px) = color?;
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p.swap(0, 2);
        if let Some((_, _, m)) = &mask {
            // マスクの黒い所が絵、白い所が透明。
            p[3] = if m[i * 4] == 0 { 255 } else { 0 };
        }
    }
    Some(Icon {
        width,
        height,
        rgba: px.into(),
    })
}

/// ビットマップを上から下の並びの BGRA で読み出す（一辺 256 まで）。
unsafe fn bitmap_bgra(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    {
        let mut bm = BITMAP::default();
        let got = unsafe {
            GetObjectW(
                bitmap.into(),
                size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as _),
            )
        };
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if got == 0 || w <= 0 || h <= 0 || w > 256 || h > 256 {
            return None;
        }
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // 負の高さで上から下の並びにする。
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let dc = unsafe { GetDC(None) };
        let lines = unsafe {
            GetDIBits(
                dc,
                bitmap,
                0,
                h as u32,
                Some(bgra.as_mut_ptr() as _),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        unsafe { ReleaseDC(None, dc) };
        (lines == h).then_some((w as u32, h as u32, bgra))
    }
}

/// スタートメニューのアプリ（`shell:AppsFolder`）を並べる。名前・AppUserModelId・ショートカットの指す実行ファイル名と、
/// アイコン（64px）を読む。
pub(super) fn installed_apps() -> Result<Vec<InstalledApp>, Error> {
    // シェルの項目は STA で読む（このワーカーのスレッドだけで使う）。
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) }?;
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems) }?;
    let take = |s: ::windows::core::Result<PWSTR>| {
        let s = s.ok()?;
        let text = unsafe { s.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(s.0 as *const _)) };
        text
    };
    let mut apps = Vec::new();
    loop {
        let mut item = [None];
        let mut fetched = 0u32;
        if unsafe { items.Next(&mut item, Some(&mut fetched as *mut u32)) }.is_err() || fetched == 0
        {
            break;
        }
        let Some(item) = item[0].take().and_then(|i| i.cast::<IShellItem2>().ok()) else {
            continue;
        };
        let (Some(name), Some(id)) = (
            take(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }),
            take(unsafe { item.GetString(&PKEY_AppUserModel_ID) }),
        ) else {
            continue;
        };
        let exe = take(unsafe { item.GetString(&PKEY_Link_TargetParsingPath) })
            .filter(|p| p.to_ascii_lowercase().ends_with(".exe"))
            .and_then(|p| p.rsplit('\\').next().map(str::to_owned));
        let icon = item
            .cast::<IShellItemImageFactory>()
            .ok()
            .and_then(|f| {
                unsafe {
                    f.GetImage(
                        SIZE { cx: 64, cy: 64 },
                        SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
                    )
                }
                .ok()
            })
            .and_then(|bitmap| {
                let read = unsafe { bitmap_bgra(bitmap) };
                unsafe {
                    let _ = DeleteObject(bitmap.into());
                }
                let (width, height, mut px) = read?;
                // シェルの絵は乗算済みのアルファなので、ストレートに戻す。
                for p in px.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                    let a = p[3] as u32;
                    if a > 0 {
                        for c in &mut p[..3] {
                            *c = (u32::from(*c) * 255).checked_div(a).unwrap_or(255).min(255) as u8;
                        }
                    }
                }
                Some(Icon {
                    width,
                    height,
                    rgba: px.into(),
                })
            });
        apps.push(InstalledApp {
            name,
            id,
            exe,
            icon,
        });
    }
    Ok(apps)
}

/// アプリを AppUserModelId で起動する（ストアのアプリもデスクトップのアプリも `shell:AppsFolder` 経由で開ける）。
pub(super) fn launch(id: &str) -> Result<(), Error> {
    let path = ::windows::core::HSTRING::from(format!("shell:AppsFolder\\{id}"));
    let done = unsafe {
        ShellExecuteW(
            None,
            ::windows::core::w!("open"),
            &path,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // 32 以下はエラー（ShellExecute の決まり）。
    if done.0 as isize <= 32 {
        return Err(Error(format!("cannot launch {id} ({})", done.0 as isize)));
    }
    Ok(())
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
    mouse: MouseDevice,
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
            mouse: MouseDevice::default(),
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

    /// 取得元のウィンドウを前に出してフォーカスする（最小化していれば戻す）。画面なら何もしない。
    fn bring_to_front(&self) {
        if self.source.kind != SourceKind::Window {
            return;
        }
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
    }

    fn activate_target(&mut self, touch: Touch) -> Result<Option<POINT>, Error> {
        let hwnd = HWND(self.source.target as _);
        self.bring_to_front();
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
    fn focus(&mut self) -> Result<(), Error> {
        self.bring_to_front();
        Ok(())
    }

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
        self.button(touch, Button::Left)
    }

    fn button(&mut self, touch: Touch, button: Button) -> Result<(), Error> {
        let point = if touch.phase == TouchPhase::Down
            && self.mouse.pressed_by.is_none()
            && self.source.kind == SourceKind::Window
        {
            let Some(point) = self.activate_target(touch)? else {
                // 他の窓への誤入力を避ける。前面化できないクリックだけを捨て、画像配信は続ける。
                return Ok(());
            };
            point
        } else {
            normalized_point(self.bounds()?, touch.x, touch.y)
        };
        self.mouse.send(touch.id, touch.phase, point, button)
    }

    fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), Error> {
        let point = if self.source.kind == SourceKind::Window {
            let Some(point) = self.activate_target(Touch {
                id: 0,
                phase: TouchPhase::Move,
                x,
                y,
            })?
            else {
                return Ok(());
            };
            point
        } else {
            normalized_point(self.bounds()?, x, y)
        };
        self.mouse.scroll(point, dx, dy)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
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

#[derive(Default)]
struct MouseDevice {
    // shortcut: Windows のデスクトップはマウス 1 個として操作する。複数指のジェスチャーが必要になったらタッチ注入を併用する。
    pressed_by: Option<u32>,
    button: Option<Button>,
}
impl MouseDevice {
    fn button(flag: MOUSE_EVENT_FLAGS) -> Result<(), Error> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: flag,
                    ..Default::default()
                },
            },
        };
        if unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } != 1 {
            return Err(Error("desktop mouse input was rejected by Windows".into()));
        }
        Ok(())
    }

    fn flags(button: Button) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
        match button {
            Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            Button::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
            Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        }
    }

    fn send(
        &mut self,
        id: u32,
        phase: TouchPhase,
        point: POINT,
        button: Button,
    ) -> Result<(), Error> {
        match phase {
            TouchPhase::Move if self.pressed_by.is_none() || self.pressed_by == Some(id) => {
                unsafe { SetCursorPos(point.x, point.y)? };
            }
            TouchPhase::Down if self.pressed_by.is_none() => {
                unsafe { SetCursorPos(point.x, point.y)? };
                Self::button(Self::flags(button).0)?;
                self.pressed_by = Some(id);
                self.button = Some(button);
            }
            TouchPhase::Up | TouchPhase::Cancel if self.pressed_by == Some(id) => {
                if phase == TouchPhase::Up {
                    unsafe { SetCursorPos(point.x, point.y)? };
                }
                Self::button(Self::flags(self.button.unwrap_or(Button::Left)).1)?;
                self.pressed_by = None;
                self.button = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn scroll(&self, point: POINT, dx: f32, dy: f32) -> Result<(), Error> {
        unsafe { SetCursorPos(point.x, point.y)? };
        for (flag, delta) in [(MOUSEEVENTF_WHEEL, dy), (MOUSEEVENTF_HWHEEL, dx)] {
            let amount = (delta * 40.0).round() as i32;
            if amount == 0 {
                continue;
            }
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        mouseData: amount as u32,
                        dwFlags: flag,
                        ..Default::default()
                    },
                },
            };
            if unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } != 1 {
                return Err(Error(
                    "desktop mouse wheel input was rejected by Windows".into(),
                ));
            }
        }
        Ok(())
    }
}
impl Drop for MouseDevice {
    fn drop(&mut self) {
        if let Some(button) = self.button {
            let _ = Self::button(Self::flags(button).1);
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

    #[test]
    fn screens_are_named_after_their_monitors() {
        let screens = vec![
            (r"\\.\DISPLAY1".to_owned(), Some("DELL U2720Q".to_owned())),
            (r"\\.\DISPLAY2".to_owned(), Some("DELL U2720Q".to_owned())),
            (r"\\.\DISPLAY3".to_owned(), Some("LG HDR 4K".to_owned())),
            (r"\\.\DISPLAY4".to_owned(), None),
        ];
        let label = |d: &str| screen_label(d, &screens);
        assert_eq!(label(r"\\.\DISPLAY1"), "DELL U2720Q (1)");
        assert_eq!(label(r"\\.\DISPLAY3"), "LG HDR 4K");
        assert_eq!(label(r"\\.\DISPLAY4"), "Display 4");
    }
}
