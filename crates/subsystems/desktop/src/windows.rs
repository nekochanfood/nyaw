use super::{
    Button, Capabilities, Cursor, Error, Frame, Icon, InstalledApp, Popup, Session, Source,
    SourceKind, Touch, TouchPhase, Wake,
};

pub(super) const CAPABILITIES: Capabilities = Capabilities {
    hover: true,
    minimize: true,
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
                IShellItem2, IShellItemImageFactory, KF_FLAG_DEFAULT, SEE_MASK_FLAG_NO_UI,
                SEE_MASK_NOASYNC, SHCreateItemFromParsingName, SHELLEXECUTEINFOW,
                SHGetKnownFolderItem, SIGDN_NORMALDISPLAY, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
                ShellExecuteExW,
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
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { cloaked(hwnd) } {
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
    // 画面名は "\\.\DISPLAY2" 形式の装置名のため、モニター製品名（取得不可時は「Display 2」）に置換する
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

/// DWM が隠しているウィンドウ（別の仮想デスクトップにあるものなど）か。
unsafe fn cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let _ = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as _,
            size_of::<u32>() as u32,
        )
    };
    cloaked != 0
}

/// 取得元のウィンドウより手前（Z オーダーで先）にある、同じプロセスのポップアップ（[`above`] が集める）。
struct Above {
    target: HWND,
    process: u32,
    windows: Vec<(HWND, RECT)>,
}

/// [`EnumWindows`] は手前から順に呼ぶので、取得元の番で止める。メニューやツールチップは枠もタイトルも無い
/// ポップアップなので、タイトルのあるもの（ダイアログや同じアプリの別のウィンドウ）と、押せないもの（影など）は除く。
unsafe extern "system" fn above(hwnd: HWND, data: LPARAM) -> BOOL {
    let above = unsafe { &mut *(data.0 as *mut Above) };
    if hwnd == above.target {
        return false.into();
    }
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
    if process != above.process
        || !unsafe { IsWindowVisible(hwnd) }.as_bool()
        || unsafe { cloaked(hwnd) }
    {
        return true.into();
    }
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    let extended = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    if style & WS_POPUP.0 == 0
        || style & WS_CAPTION.0 == WS_CAPTION.0
        || style & WS_DISABLED.0 != 0
        || extended & WS_EX_TRANSPARENT.0 != 0
    {
        return true.into();
    }
    // 影を除いた見た目の範囲（取得元の範囲と同じ測り方）。
    let mut rect = RECT::default();
    let measured = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as _,
            size_of::<RECT>() as u32,
        )
    };
    if measured.is_ok() && rect.right > rect.left && rect.bottom > rect.top {
        above.windows.push((hwnd, rect));
    }
    true.into()
}

/// `windows` のうち、`bounds` に重なるものと、それに重なるもの（外へはみ出したサブメニュー）を、元の順のまま返す。
// ponytail: 重なりだけで決める。同じアプリの関係ない小窓が重なっていても拾う。外れる例が出たら所有者のウィンドウで絞る。
fn touching<T>(bounds: RECT, windows: Vec<(T, RECT)>) -> Vec<(T, RECT)> {
    let overlaps = |a: &RECT, b: &RECT| {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    };
    let mut kept = vec![false; windows.len()];
    let mut grew = true;
    while grew {
        grew = false;
        for i in 0..windows.len() {
            let rect = &windows[i].1;
            if !kept[i]
                && (overlaps(rect, &bounds)
                    || windows
                        .iter()
                        .zip(&kept)
                        .any(|((_, r), &k)| k && overlaps(rect, r)))
            {
                kept[i] = true;
                grew = true;
            }
        }
    }
    windows
        .into_iter()
        .zip(kept)
        .filter_map(|(w, k)| k.then_some(w))
        .collect()
}

/// GDI 装置名（`"\\.\DISPLAY2"`）から、接続先モニターの製品名（EDID 名。`"DELL U2720Q"` 等）への対応表を返す。
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

/// 画面の表示名を返す。モニター製品名が取得でき、同一名が複数あれば番号を付与する。取得不可時は「Display 番号」。
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

/// ウィンドウのアイコンを取得する。ウィンドウ固有のアイコン（WM_GETICON・クラス）が無い場合、プロセスの実行ファイルから取得する。
unsafe fn window_icon(hwnd: HWND) -> Option<Icon> {
    let mut icon = 0usize;
    // 応答停止中のウィンドウでソース一覧取得全体がブロックされないようタイムアウトを設定する
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
        // 借用アイコンのため破棄不要
        return unsafe { icon_rgba(HICON(icon as _)) }.map(|(i, _)| i);
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
    rgba.map(|(i, _)| i)
}

/// ウィンドウの所属プロセスの実行ファイルパスを UTF-16（終端ヌルなし）で返す。
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

/// アイコン・カーソルを RGBA として読み出し、ホットスポットとともに返す。アルファチャネルを持たない旧形式はマスクから透明領域を生成する。
unsafe fn icon_rgba(icon: HICON) -> Option<(Icon, (u32, u32))> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let hotspot = (info.xHotspot, info.yHotspot);
    let read = |bitmap: HBITMAP| unsafe { bitmap_bgra(bitmap) };
    let color = (!info.hbmColor.is_invalid())
        .then(|| read(info.hbmColor))
        .flatten();
    let mask = match &color {
        Some((_, _, px)) if px.as_chunks::<4>().0.iter().any(|p| p[3] != 0) => None,
        _ => read(info.hbmMask),
    };
    unsafe {
        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
    }
    let Some((width, height, mut px)) = color else {
        // 白黒カーソル：マスクの上半分が AND マスク、下半分が XOR マスク
        let (width, double, m) = mask?;
        let n = (width * (double / 2)) as usize;
        let rgba: Vec<u8> = (0..n)
            .flat_map(|i| match (m[i * 4] != 0, m[(n + i) * 4] != 0) {
                (true, false) => [0, 0, 0, 0],
                (false, true) => [255; 4],
                // ponytail: 反転ピクセル（AND・XOR ともに 1）は背景色が不明なため黒で代替する。視認性が不足する場合は縁取りを追加する
                _ => [0, 0, 0, 255],
            })
            .collect();
        let image = Icon {
            width,
            height: double / 2,
            rgba: rgba.into(),
        };
        return Some((image, hotspot));
    };
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p.swap(0, 2);
        if let Some((_, _, m)) = &mask {
            // マスクの黒が不透明領域、白が透明領域
            p[3] = if m[i * 4] == 0 { 255 } else { 0 };
        }
    }
    Some((
        Icon {
            width,
            height,
            rgba: px.into(),
        },
        hotspot,
    ))
}

/// ビットマップをトップダウン順の BGRA として読み出す（一辺 256px 以下に限定）。
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
                // 負の高さを指定してトップダウン順にする
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

/// スタートメニューのアプリ（`shell:AppsFolder`）を列挙する。名前・AppUserModelId・ショートカット先の実行ファイル名、
/// およびアイコン（64px）を取得する。
pub(super) fn installed_apps() -> Result<Vec<InstalledApp>, Error> {
    // シェル項目は STA で取得する（このワーカースレッド内でのみ使用）
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) }?;
    let items: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems) }?;
    let mut apps = Vec::new();
    loop {
        let mut item = [None];
        let mut fetched = 0u32;
        if unsafe { items.Next(&mut item, Some(&mut fetched as *mut u32)) }.is_err() || fetched == 0
        {
            break;
        }
        if let Some(app) = item[0]
            .take()
            .and_then(|i| i.cast::<IShellItem2>().ok())
            .and_then(|i| app_of(&i, None))
        {
            apps.push(app);
        }
    }
    Ok(apps)
}

/// AppUserModelId `id` のスタートメニューのアプリ。シェル項目の取得に STA が必要なため、専用スレッドで実行する（[`sta`]）。
pub(super) fn installed_app(id: &str) -> Option<InstalledApp> {
    sta(|| {
        let path = ::windows::core::HSTRING::from(format!("shell:AppsFolder\\{id}"));
        let item: IShellItem2 = unsafe { SHCreateItemFromParsingName(&path, None) }.ok()?;
        app_of(&item, Some(id.to_owned()))
    })
    .flatten()
}

/// シェル項目 `item` のアプリ。ID は `id`（`None` なら項目の AppUserModelId）。名前か ID が無ければ `None`。
fn app_of(item: &IShellItem2, id: Option<String>) -> Option<InstalledApp> {
    let name = take(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) })?;
    let id = id.or_else(|| take(unsafe { item.GetString(&PKEY_AppUserModel_ID) }))?;
    let exe = take(unsafe { item.GetString(&PKEY_Link_TargetParsingPath) })
        .filter(|p| p.to_ascii_lowercase().ends_with(".exe"))
        .and_then(|p| p.rsplit('\\').next().map(str::to_owned));
    Some(InstalledApp {
        name,
        id,
        exe,
        icon: shell_icon(item),
    })
}

/// シェルが確保した文字列を読み取り、解放する。
fn take(s: ::windows::core::Result<PWSTR>) -> Option<String> {
    let s = s.ok()?;
    let text = unsafe { s.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(s.0 as *const _)) };
    text
}

/// `f` を STA の専用スレッドで実行し、終わるまで待つ。シェルの API は STA を前提とするものがあるため、呼び出し元スレッド
/// （通知のワーカーは MTA）の COM の初期化状態に依存しないようにする。スレッドが異常終了したら `None`。
fn sta<T: Send>(f: impl FnOnce() -> T + Send) -> Option<T> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            f()
        })
        .join()
        .ok()
    })
}

/// シェル項目のアイコン（64px）を取得する。
fn shell_icon(item: &IShellItem2) -> Option<Icon> {
    let factory = item.cast::<IShellItemImageFactory>().ok()?;
    let bitmap = unsafe {
        factory.GetImage(
            SIZE { cx: 64, cy: 64 },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )
    }
    .ok()?;
    let read = unsafe { bitmap_bgra(bitmap) };
    unsafe {
        let _ = DeleteObject(bitmap.into());
    }
    let (width, height, mut px) = read?;
    // シェルアイコンは事前乗算アルファのため、ストレートアルファに逆変換する
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
}

/// アプリ（AppUserModelId）のスタートメニューアイコンを取得する。シェル項目の取得に STA が必要なため、専用スレッドで実行する（[`sta`]）。
pub(super) fn app_icon(id: &str) -> Option<Icon> {
    sta(|| {
        let path = ::windows::core::HSTRING::from(format!("shell:AppsFolder\\{id}"));
        let item: IShellItem2 = unsafe { SHCreateItemFromParsingName(&path, None) }.ok()?;
        shell_icon(&item)
    })
    .flatten()
}

/// AppUserModelId を指定してアプリを起動する（ストアアプリ・デスクトップアプリとも `shell:AppsFolder` 経由で起動可能）。
/// 専用スレッド（[`sta`]）は起動を依頼したらすぐ終わるため、起動の処理が終わるまで待つ指定（`SEE_MASK_NOASYNC`）を付ける。
/// 失敗は呼び出し側が表示するので、OS のエラーダイアログ（VR からは見えない）は出さない（`SEE_MASK_FLAG_NO_UI`）。
pub(super) fn launch(id: &str) -> Result<(), Error> {
    sta(|| {
        let path = ::windows::core::HSTRING::from(format!("shell:AppsFolder\\{id}"));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: ::windows::core::w!("open"),
            lpFile: PCWSTR(path.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| Error(format!("cannot launch {id}: {e}")))
    })
    .unwrap_or_else(|| {
        Err(Error(format!(
            "cannot launch {id}: the launcher thread failed"
        )))
    })
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

/// 取り込みに使う D3D の装置。取得元とメニューの取り込みで共用する。
struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    direct: IDirect3DDevice,
}

/// 1 つの取り込み対象（取得元か、その手前のメニュー）の WGC セッションと、届いた画像の読み出し。
struct Grab {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    _item: GraphicsCaptureItem,
    size: SizeInt32,
    staging: Option<ID3D11Texture2D>,
    /// 一時停止中に保持した最新フレーム（[`Session::skip`]）。プール内の 1 スロットを占有するが、残りの 1 スロットで次フレームを受信できる。
    held: Option<::windows::Graphics::Capture::Direct3D11CaptureFrame>,
    ready: Arc<AtomicBool>,
}

/// 出ているメニュー 1 つの取り込み。
struct Menu {
    hwnd: HWND,
    /// 取り込みを始められなかったメニューは `None`（出ている間は試し直さない）。
    grab: Option<Grab>,
    /// 最後に届いた画像。最初の画像が届くまでは `None`。
    image: Option<Icon>,
}

pub(super) struct Capture {
    grab: Grab,
    gpu: Gpu,
    closed: Arc<AtomicBool>,
    source: Source,
    mouse: MouseDevice,
    /// 最後に取得したカーソルのハンドルと画像（同一ハンドルであれば再取得を省略する）。
    cursor: Option<(isize, Option<Arc<Cursor>>)>,
    /// 取得元の手前に出ているメニュー（手前から順）。押した所がメニューなら、取得元でなくても入力を送る。
    menus: Vec<Menu>,
    _apartment: Apartment,
}

impl Capture {
    pub fn new(source: Source, wake: Wake) -> Result<Self, Error> {
        // WinRT の静的キャッシュと MTA のライフタイムを一致させ、全セッション停止後の再開時にも参照を有効に保つ
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
        // WGC とポインタ入力の双方をデバイスピクセルで処理し、異なる DPI のモニター間でも座標を統一する
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        if source.kind == SourceKind::Window
            && unsafe { IsIconic(HWND(source.target as _)) }.as_bool()
        {
            // 最小化されたウィンドウでは WGC がフレームを生成せず初回フレーム待ちで停止するため、復元する
            unsafe {
                let _ = ShowWindowAsync(HWND(source.target as _), SW_RESTORE);
            }
        }
        if source.kind == SourceKind::Window {
            let hwnd = HWND(source.target as _);
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool()
                || !unsafe { IsWindowVisible(hwnd) }.as_bool()
            {
                return Err(Error(super::failure::WINDOW_HIDDEN.into()));
            }
        }
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(Error(super::failure::UNSUPPORTED.into()));
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
            return Err(Error(super::failure::NO_PIXELS.into()));
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
        let device: ID3D11Device =
            device.ok_or_else(|| Error(super::failure::D3D_DEVICE.into()))?;
        let context = context.ok_or_else(|| Error(super::failure::D3D_CONTEXT.into()))?;
        let dxgi: IDXGIDevice = device.cast()?;
        let direct: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let closed = Arc::new(AtomicBool::new(false));
        let signal = closed.clone();
        item.Closed(&TypedEventHandler::new(move |_, _| {
            signal.store(true, Ordering::Release);
            Ok(())
        }))?;
        let gpu = Gpu {
            device,
            context,
            direct,
        };
        let grab = Grab::open(&gpu, item, size, Some(wake))?;
        Ok(Self {
            grab,
            gpu,
            closed,
            source,
            mouse: MouseDevice::default(),
            cursor: None,
            menus: Vec::new(),
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
                        return Err(Error(super::failure::WINDOW_MINIMIZED.into()));
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
            return Err(Error(super::failure::INVALID_BOUNDS.into()));
        }
        Ok(rect)
    }

    /// 取得元ウィンドウを前面に出してフォーカスする（最小化時は復元する）。画面ソースの場合は何もしない。
    fn bring_to_front(&self) {
        if self.source.kind != SourceKind::Window {
            return;
        }
        let hwnd = HWND(self.source.target as _);
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            }
            let _ = BringWindowToTop(hwnd);
            if !SetForegroundWindow(hwnd).as_bool() || GetForegroundWindow() != hwnd {
                // VR 入力のみでは前面化の権限が付与されない場合がある。切り替え時に一時的に入力キューを共有する
                let current = GetCurrentThreadId();
                let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
                let target = GetWindowThreadProcessId(hwnd, None);
                let mut message = MSG::default();
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                let joined_foreground = foreground != 0
                    && foreground != current
                    && AttachThreadInput(current, foreground, true).as_bool();
                let joined_target = target != 0
                    && target != current
                    && target != foreground
                    && AttachThreadInput(current, target, true).as_bool();
                let _ = BringWindowToTop(hwnd);
                let _ = SetActiveWindow(hwnd);
                let _ = SetForegroundWindow(hwnd);
                if joined_target {
                    let _ = AttachThreadInput(current, target, false);
                }
                if joined_foreground {
                    let _ = AttachThreadInput(current, foreground, false);
                }
            }
            if GetForegroundWindow() != hwnd {
                // フォーカス権限が拒否された場合でも、Z オーダー上の重なりは前面に移動する
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
            }
        }
    }

    fn activate_target(&mut self, touch: Touch) -> Result<Option<POINT>, Error> {
        let hwnd = HWND(self.source.target as _);
        self.bring_to_front();
        // 前面化は対象の UI スレッドで非同期に反映されるため、即座の判定では拒否しない
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
        loop {
            if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                return Ok(None);
            }
            if let Ok(bounds) = self.bounds() {
                let point = normalized_point(bounds, touch.x, touch.y);
                let root = unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) };
                if root == hwnd || self.menus.iter().any(|m| m.hwnd == root) {
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
}

impl Grab {
    /// `hwnd`（メニュー）だけを取り込む。後ろのウィンドウは映らず、ウィンドウの透明な所は透明で届く。
    fn window(gpu: &Gpu, hwnd: HWND) -> Result<Self, Error> {
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd) }?;
        let size = item.Size()?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(Error(super::failure::NO_PIXELS.into()));
        }
        let grab = Self::open(gpu, item, size, None)?;
        // メニューの周りに取り込み中の黄色い枠を出さない。非対応の OS では枠が出る。
        let _ = grab.session.SetIsBorderRequired(false);
        Ok(grab)
    }

    /// `item` の取り込みを始める。画像の到着で `wake` があればワーカーを起こす。
    fn open(
        gpu: &Gpu,
        item: GraphicsCaptureItem,
        size: SizeInt32,
        wake: Option<Wake>,
    ) -> Result<Self, Error> {
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &gpu.direct,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let ready = Arc::new(AtomicBool::new(false));
        let signal = ready.clone();
        pool.FrameArrived(&TypedEventHandler::new(move |_, _| {
            signal.store(true, Ordering::Release);
            if let Some(wake) = &wake {
                wake.wake();
            }
            Ok(())
        }))?;
        let session = pool.CreateCaptureSession(&item)?;
        // 既定では OS がリフレッシュレートより粗くフレームを間引く。毎 vsync でフレームを生成させ、FPS 上限はワーカー側で制御する
        // （上限間隔を指定すると vsync の倍数に切り上げられ、143Hz モニターで 60fps を指定すると 48fps になる）。
        // Windows 11 24H2 より前にはこの API が存在しないため、失敗時は既定動作のまま続行する
        let _ = session.SetMinUpdateInterval(TimeSpan { Duration: 10_000 });
        // カーソルはキャプチャ映像に含めず、上位レイヤーがポインタとして描画する（Windows 10 2004 より前ではこの設定は無効）
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture()?;
        Ok(Self {
            session,
            pool,
            _item: item,
            size,
            staging: None,
            held: None,
            ready,
        })
    }

    fn next_frame(
        &self,
    ) -> Result<Option<::windows::Graphics::Capture::Direct3D11CaptureFrame>, Error> {
        // WinRT の正常な null 戻り値を「フレームなし」として処理する。非 nullable な Rust API は Error(0) に変換するため、直接 vtable を呼ぶ
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

    /// 利用可能なフレームのうち最新のもの（保持分を含む）を返す。古いフレームは Close してプールに返却する。
    fn latest(
        &mut self,
    ) -> Result<Option<::windows::Graphics::Capture::Direct3D11CaptureFrame>, Error> {
        let mut latest = self.held.take();
        if !self.ready.swap(false, Ordering::AcqRel) {
            return Ok(latest);
        }
        // プール容量は 2 フレームのため、低 FPS や一時停止で蓄積された古いフレームを破棄し最新のみ保持する
        while let Some(newer) = self.next_frame()? {
            if let Some(older) = latest.replace(newer) {
                older.Close()?;
            }
        }
        Ok(latest)
    }

    /// 新しい画像が届いていれば読み出す。画素は事前乗算アルファの RGBA で、透明な所はそのまま透明になる。
    fn read(&mut self, gpu: &Gpu) -> Result<Option<Frame>, Error> {
        let Some(frame) = self.latest()? else {
            return Ok(None);
        };
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
                    gpu.device
                        .CreateTexture2D(&desc, None, Some(&mut self.staging))?;
                }
            }
            let staging = self
                .staging
                .as_ref()
                .ok_or_else(|| Error(super::failure::D3D_STAGING.into()))?;
            unsafe {
                gpu.context.CopyResource(staging, &texture);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                gpu.context
                    .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                let bytes = (mapped.RowPitch as usize).checked_mul(height as usize);
                let result = if mapped.pData.is_null()
                    || bytes.is_none_or(|n| n > 256 * 1024 * 1024 || n > isize::MAX as usize)
                {
                    Err(Error(super::failure::INVALID_TEXTURE.into()))
                } else {
                    Frame::from_bgra(
                        width,
                        height,
                        mapped.RowPitch as usize,
                        std::slice::from_raw_parts(mapped.pData as *const u8, bytes.unwrap()),
                    )
                };
                gpu.context.Unmap(staging, 0);
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
        // リサイズ中のフレームは破棄し、フレーム参照を解放してからプールを再生成する
        if self.staging.is_none() {
            self.pool.Recreate(
                &gpu.direct,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                self.size,
            )?;
        }
        result
    }
}

impl Drop for Grab {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

impl Session for Capture {
    fn focus(&mut self) -> Result<(), Error> {
        self.bring_to_front();
        Ok(())
    }

    fn minimize(&mut self) -> Result<(), Error> {
        if self.source.kind == SourceKind::Window {
            unsafe {
                let _ = ShowWindowAsync(HWND(self.source.target as _), SW_MINIMIZE);
            }
        }
        Ok(())
    }

    fn minimized(&self) -> bool {
        self.source.kind == SourceKind::Window
            && unsafe { IsIconic(HWND(self.source.target as _)) }.as_bool()
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            || (self.source.kind == SourceKind::Window
                && (!unsafe { IsWindow(Some(HWND(self.source.target as _))) }.as_bool()
                    || !unsafe { IsWindowVisible(HWND(self.source.target as _)) }.as_bool()
                        && !self.minimized()))
    }

    fn cursor(&mut self) -> Option<Arc<Cursor>> {
        let mut info = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetCursorInfo(&mut info) }.ok()?;
        if info.flags.0 & CURSOR_SHOWING.0 == 0 || info.hCursor.is_invalid() {
            return None;
        }
        let handle = info.hCursor.0 as isize;
        if let Some((h, cursor)) = &self.cursor
            && *h == handle
        {
            return cursor.clone();
        }
        // ponytail: アニメーションカーソル（待機スピナー等）は先頭フレームのみ取得。フレームアニメーションが必要になったら DrawIconEx で描画する
        let cursor = unsafe { icon_rgba(HICON(info.hCursor.0)) }
            .map(|(image, hotspot)| Arc::new(Cursor { image, hotspot }));
        self.cursor = Some((handle, cursor.clone()));
        cursor
    }

    fn popups(&mut self) -> Vec<Popup> {
        let target = HWND(self.source.target as _);
        let Some(bounds) = (self.source.kind == SourceKind::Window)
            .then(|| self.bounds().ok())
            .flatten()
        else {
            self.menus.clear();
            return Vec::new();
        };
        let mut above = Above {
            target,
            process: 0,
            windows: Vec::new(),
        };
        unsafe { GetWindowThreadProcessId(target, Some(&mut above.process)) };
        if above.process != 0 {
            // 取得元の番で列挙を止めると失敗が返るが、それまでに集めたものを使う。
            let _ =
                unsafe { EnumWindows(Some(self::above), LPARAM(&mut above as *mut _ as isize)) };
        }
        let (width, height) = (
            (bounds.right - bounds.left) as f32,
            (bounds.bottom - bounds.top) as f32,
        );
        let mut old = std::mem::take(&mut self.menus);
        let mut popups = Vec::new();
        for (hwnd, r) in touching(bounds, above.windows) {
            let mut menu = match old.iter().position(|m| m.hwnd == hwnd) {
                Some(i) => old.swap_remove(i),
                None => Menu {
                    hwnd,
                    grab: Grab::window(&self.gpu, hwnd).ok(),
                    image: None,
                },
            };
            // 読み出せなかったメニューは最後の画像のままにし、取得元の配信は止めない（閉じる途中のメニューは失敗しうる）。
            if let Some(Ok(Some(frame))) = menu.grab.as_mut().map(|g| g.read(&self.gpu)) {
                menu.image = Some(Icon {
                    width: frame.width,
                    height: frame.height,
                    rgba: frame.rgba.into(),
                });
            }
            if let Some(image) = &menu.image {
                popups.push(Popup {
                    id: hwnd.0 as usize as u64,
                    area: [
                        (r.left - bounds.left) as f32 / width,
                        (r.top - bounds.top) as f32 / height,
                        (r.right - r.left) as f32 / width,
                        (r.bottom - r.top) as f32 / height,
                    ],
                    image: image.clone(),
                });
            }
            self.menus.push(menu);
        }
        popups
    }

    fn skip(&mut self) -> Result<(), Error> {
        self.grab.held = self.grab.latest()?;
        Ok(())
    }

    fn frame(&mut self) -> Result<Option<Frame>, Error> {
        self.grab.read(&self.gpu)
    }

    fn touch(&mut self, touch: Touch) -> Result<(), Error> {
        self.button(touch, Button::Left)
    }

    fn button(&mut self, touch: Touch, button: Button) -> Result<(), Error> {
        if self.minimized() {
            return Ok(());
        }
        let point = if touch.phase == TouchPhase::Down
            && self.mouse.pressed_by.is_none()
            && self.source.kind == SourceKind::Window
        {
            let Some(point) = self.activate_target(touch)? else {
                // 他ウィンドウへの誤入力を防止する。前面化できないクリックのみ破棄し、フレーム配信は継続する
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

/// WGC のフレーム時刻（QPC を 100ns 単位に変換した値）を `Instant` に変換する。
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

/// 仮想デスクトップ上のピクセル座標を `MOUSEEVENTF_ABSOLUTE` の 0..=65535 範囲にマッピングする。
fn virtual_desk_point(point: POINT, left: i32, top: i32, width: i32, height: i32) -> (i32, i32) {
    let scale = |v: i32, origin: i32, size: i32| {
        // 四捨五入する（切り捨てでは Windows がピクセルに逆変換した際に 1px 手前にずれる）
        let span = (size - 1).max(1) as i64;
        (((v - origin) as i64 * 65535 + span / 2) / span).clamp(0, 65535) as i32
    };
    (scale(point.x, left, width), scale(point.y, top, height))
}

fn normalized_point(bounds: RECT, x: f32, y: f32) -> POINT {
    POINT {
        x: bounds.left + ((bounds.right - bounds.left - 1) as f32 * x).round() as i32,
        y: bounds.top + ((bounds.bottom - bounds.top - 1) as f32 * y).round() as i32,
    }
}

#[derive(Default)]
struct MouseDevice {
    // ponytail: Windows デスクトップはマウス 1 台として操作する。マルチタッチジェスチャーが必要になったらタッチ注入を併用する
    pressed_by: Option<u32>,
    button: Option<Button>,
    /// 1 ノッチ未満のホイール量の蓄積値（縦, 横）。
    wheel: [f32; 2],
}

/// スティック入力量を蓄積し、送信可能なノッチ数（`WHEEL_DELTA` の倍数）を返す。
/// 120 未満の量を整数除算で切り捨てるアプリが存在する（コンソールターミナル、Win32 スライダー等）。
fn wheel_notches(acc: &mut f32, delta: f32) -> i32 {
    const WHEEL_DELTA: f32 = 120.0;
    let amount = delta * 40.0;
    // 方向が反転した場合、逆方向の蓄積値を即座に破棄する
    if *acc * amount < 0.0 {
        *acc = 0.0;
    }
    *acc += amount;
    let notches = (*acc / WHEEL_DELTA).trunc();
    *acc -= notches * WHEEL_DELTA;
    (notches * WHEEL_DELTA) as i32
}
impl MouseDevice {
    fn input(mi: MOUSEINPUT) -> Result<(), Error> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi },
        };
        if unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } != 1 {
            return Err(Error("desktop mouse input was rejected by Windows".into()));
        }
        Ok(())
    }

    fn button(flag: MOUSE_EVENT_FLAGS) -> Result<(), Error> {
        Self::input(MOUSEINPUT {
            dwFlags: flag,
            ..Default::default()
        })
    }

    /// カーソル移動も入力イベントとして注入する。`SetCursorPos` のみではボタン押下中の移動を受け取らないアプリが存在する（ターミナルの範囲選択等）。
    fn move_to(point: POINT) -> Result<(), Error> {
        let (left, top, width, height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let (dx, dy) = virtual_desk_point(point, left, top, width, height);
        Self::input(MOUSEINPUT {
            dx,
            dy,
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            ..Default::default()
        })
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
                Self::move_to(point)?;
            }
            TouchPhase::Down if self.pressed_by.is_none() => {
                Self::move_to(point)?;
                Self::button(Self::flags(button).0)?;
                self.pressed_by = Some(id);
                self.button = Some(button);
            }
            TouchPhase::Up | TouchPhase::Cancel if self.pressed_by == Some(id) => {
                if phase == TouchPhase::Up {
                    Self::move_to(point)?;
                }
                Self::button(Self::flags(self.button.unwrap_or(Button::Left)).1)?;
                self.pressed_by = None;
                self.button = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn scroll(&mut self, point: POINT, dx: f32, dy: f32) -> Result<(), Error> {
        Self::move_to(point)?;
        for (i, flag, delta) in [(0, MOUSEEVENTF_WHEEL, dy), (1, MOUSEEVENTF_HWHEEL, dx)] {
            let amount = wheel_notches(&mut self.wheel[i], delta);
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

    /// 存在しないアプリの起動は、OS のダイアログを出さずにエラーを返す。
    #[test]
    fn launching_an_unknown_app_fails_quietly() {
        assert!(launch("nyaw.test.no-such-app").is_err());
    }

    /// 実機検証: 電卓を ID で引き、MTA のスレッド（通知のワーカーと同じ）から起動する。起動した電卓は手で閉じる。
    /// `cargo test -p desktop -- --ignored launches_an_app_from_an_mta_thread`
    #[test]
    #[ignore]
    fn launches_an_app_from_an_mta_thread() {
        let id = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
        assert!(installed_app(id).is_some(), "the calculator is installed");
        std::thread::spawn(move || {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            launch(id)
        })
        .join()
        .unwrap()
        .unwrap();
    }

    /// 取得元に重なるメニューと、その外へはみ出したサブメニューを拾い、離れた小窓と縁に接するだけのもの（影など）は拾わない。
    #[test]
    fn popups_are_the_ones_touching_the_window_or_another_popup() {
        let rect = |left, top, right, bottom| RECT {
            left,
            top,
            right,
            bottom,
        };
        let window = rect(0, 0, 100, 100);
        let found = touching(
            window,
            vec![
                ("submenu", rect(150, 60, 220, 120)),
                ("far", rect(300, 300, 320, 320)),
                ("menu", rect(80, 50, 160, 150)),
                ("edge", rect(-10, 0, 0, 100)),
            ],
        );
        let names: Vec<_> = found.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["submenu", "menu"]);
    }

    #[test]
    fn wheel_sends_whole_notches_only() {
        let mut acc = 0.0;
        let sent: Vec<_> = (0..6).map(|_| wheel_notches(&mut acc, 1.0)).collect();
        assert_eq!(sent, [0, 0, 120, 0, 0, 120]);
        // 小さな入力でも蓄積すればノッチに到達する
        let mut acc = 0.0;
        assert_eq!(
            (0..12).map(|_| wheel_notches(&mut acc, 0.25)).sum::<i32>(),
            120
        );
        // 逆方向に入力した場合、以前の蓄積値は保持しない
        let mut acc = 0.0;
        wheel_notches(&mut acc, 1.0);
        wheel_notches(&mut acc, 1.0);
        assert_eq!(wheel_notches(&mut acc, -1.0), 0);
        assert_eq!(acc, -40.0);
    }

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
    fn maps_pixels_onto_absolute_virtual_desk() {
        let at = |x, y| virtual_desk_point(POINT { x, y }, -1920, -100, 3840, 1180);
        assert_eq!(at(-1920, -100), (0, 0));
        assert_eq!(at(1919, 1079), (65535, 65535));
        assert_eq!(at(-1, 0).0, 32759);
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
