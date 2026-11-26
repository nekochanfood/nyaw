//! 実デスクトップを使用するため、`--ignored` を明示的に指定した場合にのみ実行される。
#![cfg(windows)]

use desktop::{Source, SourceKind, Sources, Status, Stream, Touch, TouchPhase};
use std::{
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
        Graphics::Gdi::*,
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
            Input::Pointer::GetPointerTouchInfo,
            WindowsAndMessaging::*,
        },
    },
    core::w,
};

static TOUCHES: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static TARGETS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
static NATIVE_INPUT: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "captures the real desktop with WGC; run explicitly on an interactive Windows desktop"]
fn wgc_screen_produces_frames() {
    let sources = Sources::refresh().unwrap();
    let mut list = None;
    until(|| {
        list = sources.poll();
        list.is_some()
    });
    let source = list
        .unwrap()
        .unwrap()
        .into_iter()
        .find(|s| s.kind == SourceKind::Screen)
        .unwrap();
    let stream = Stream::start(source).unwrap();
    let mut frame = None;
    until(|| {
        if let Some(update) = stream.poll() {
            assert!(
                !matches!(update.status, Status::Failed(_) | Status::Closed),
                "{:?}",
                update.status
            );
            frame = update.frame;
        }
        frame.is_some()
    });
    let frame = frame.unwrap();
    assert!(frame.width > 0 && frame.height > 0);
    assert_eq!(
        frame.rgba.len(),
        frame.width as usize * frame.height as usize * 4
    );
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if matches!(message, WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP) {
        let mut info = Default::default();
        if unsafe { GetPointerTouchInfo((wparam.0 & 0xffff) as u32, &mut info) }.is_ok() {
            TOUCHES.lock().unwrap().push(message);
            TARGETS.lock().unwrap().push(hwnd.0 as isize);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct TestWindow(Option<HWND>);
impl Drop for TestWindow {
    fn drop(&mut self) {
        if let Some(hwnd) = self.0.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

/// 取得元の一覧から、タイトルが `name` のウィンドウを探す。
#[track_caller]
fn window_source(name: &str) -> Source {
    let sources = Sources::refresh().unwrap();
    let mut list = None;
    until(|| {
        list = sources.poll();
        list.is_some()
    });
    list.unwrap()
        .unwrap()
        .into_iter()
        .find(|s| s.name == name)
        .expect("test window in source list")
}

#[track_caller]
fn until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if check() {
            break;
        }
        assert!(Instant::now() < deadline, "desktop E2E timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "opens a real window, captures WGC frames and injects OS touch; run on an interactive Windows desktop"]
fn wgc_window_resize_native_touch_and_source_close() {
    let _guard = NATIVE_INPUT.lock().unwrap();
    TOUCHES.lock().unwrap().clear();
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            lpszClassName: w!("nyaw.desktop.e2e"),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&class), 0);
    }
    let mut window = TestWindow(Some(unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            w!("nyaw.desktop.e2e"),
            w!("nyaw desktop capture E2E"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            360,
            240,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    }));
    let hwnd = window.0.unwrap();
    let list = Sources::refresh().unwrap();
    let mut sources = None;
    until(|| {
        sources = list.poll();
        sources.is_some()
    });
    let sources = sources.unwrap().unwrap();
    assert!(sources.iter().any(|s| s.kind == SourceKind::Screen));
    let source = sources
        .into_iter()
        .find(|s| s.kind == SourceKind::Window && s.name == "nyaw desktop capture E2E")
        .expect("test window in source list");
    let stream = Stream::start(source).unwrap();
    let mut first = None;
    until(|| {
        if let Some(update) = stream.poll() {
            assert!(
                !matches!(update.status, Status::Failed(_)),
                "{:?}",
                update.status
            );
            first = update.frame;
        }
        first.is_some()
    });
    let first = first.unwrap();
    assert_eq!(
        first.rgba.len(),
        first.width as usize * first.height as usize * 4
    );
    let center = ((first.height / 2 * first.width + first.width / 2) * 4) as usize;
    assert_eq!(
        &first.rgba[center..center + 4],
        &[255; 4],
        "white test window captured as RGBA"
    );
    unsafe {
        SetWindowPos(hwnd, None, 100, 100, 600, 400, SWP_NOZORDER).unwrap();
    }
    until(|| {
        stream
            .poll()
            .and_then(|u| {
                assert!(!matches!(u.status, Status::Failed(_)), "{:?}", u.status);
                u.frame
            })
            .is_some_and(|f| f.width > first.width && f.height > first.height)
    });
    stream
        .touch(Touch {
            id: 0,
            phase: TouchPhase::Down,
            x: 0.5,
            y: 0.5,
        })
        .unwrap();
    until(|| TOUCHES.lock().unwrap().contains(&WM_POINTERDOWN));
    stream
        .touch(Touch {
            id: 0,
            phase: TouchPhase::Move,
            x: 0.55,
            y: 0.55,
        })
        .unwrap();
    until(|| TOUCHES.lock().unwrap().contains(&WM_POINTERUPDATE));
    stream
        .touch(Touch {
            id: 0,
            phase: TouchPhase::Up,
            x: 0.55,
            y: 0.55,
        })
        .unwrap();
    until(|| TOUCHES.lock().unwrap().contains(&WM_POINTERUP));
    unsafe {
        DestroyWindow(window.0.take().unwrap()).unwrap();
    }
    until(|| {
        stream.poll().is_some_and(|u| {
            assert!(!matches!(u.status, Status::Failed(_)), "{:?}", u.status);
            u.status == Status::Closed
        })
    });
}

#[test]
#[ignore = "uses isolated native windows, WGC and OS touch to verify activation and occlusion"]
fn occluded_window_activation_and_rejected_touch_do_not_stop_capture() {
    let _guard = NATIVE_INPUT.lock().unwrap();
    TOUCHES.lock().unwrap().clear();
    TARGETS.lock().unwrap().clear();
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            lpszClassName: w!("nyaw.desktop.visibility.e2e"),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&class), 0);
    }
    let create = |title, ex_style| {
        TestWindow(Some(unsafe {
            CreateWindowExW(
                ex_style,
                w!("nyaw.desktop.visibility.e2e"),
                title,
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                400,
                300,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        }))
    };
    let mut source_window = create(
        w!("nyaw desktop visibility source"),
        WINDOW_EX_STYLE::default(),
    );
    let hwnd = source_window.0.unwrap();
    let source = window_source("nyaw desktop visibility source");
    let stream = Stream::start(source).unwrap();
    let poll_frame = || {
        stream.poll().is_some_and(|u| {
            assert!(
                !matches!(u.status, Status::Failed(_) | Status::Closed),
                "{:?}",
                u.status
            );
            u.frame.is_some()
        })
    };
    until(poll_frame);
    let mut cover = create(
        w!("nyaw desktop visibility cover"),
        WINDOW_EX_STYLE::default(),
    );
    unsafe {
        let _ = SetForegroundWindow(cover.0.unwrap());
    }
    let down = Touch {
        id: 0,
        phase: TouchPhase::Down,
        x: 0.5,
        y: 0.5,
    };
    stream.touch(down).unwrap();
    until(|| {
        poll_frame();
        TOUCHES.lock().unwrap().contains(&WM_POINTERDOWN)
    });
    assert!(
        TARGETS
            .lock()
            .unwrap()
            .iter()
            .all(|&t| t == hwnd.0 as isize)
    );
    stream
        .touch(Touch {
            phase: TouchPhase::Up,
            ..down
        })
        .unwrap();
    until(|| TOUCHES.lock().unwrap().contains(&WM_POINTERUP));
    TOUCHES.lock().unwrap().clear();
    TARGETS.lock().unwrap().clear();
    unsafe {
        SetWindowPos(
            cover.0.unwrap(),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
    }
    stream.touch(down).unwrap();
    let deadline = Instant::now() + Duration::from_millis(350);
    until(|| Instant::now() >= deadline);
    assert!(
        TARGETS.lock().unwrap().is_empty(),
        "a blocked click must never hit the covering window"
    );
    stream
        .touch(Touch {
            phase: TouchPhase::Up,
            ..down
        })
        .unwrap();
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            100,
            100,
            420,
            300,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .unwrap();
    }
    until(poll_frame);
    unsafe {
        DestroyWindow(cover.0.take().unwrap()).unwrap();
    }
    stream.touch(down).unwrap();
    until(|| TOUCHES.lock().unwrap().contains(&WM_POINTERDOWN));
    assert!(
        TARGETS
            .lock()
            .unwrap()
            .iter()
            .all(|&t| t == hwnd.0 as isize)
    );
    stream
        .touch(Touch {
            phase: TouchPhase::Up,
            ..down
        })
        .unwrap();
    until(|| TOUCHES.lock().unwrap().contains(&WM_POINTERUP));
    unsafe {
        DestroyWindow(source_window.0.take().unwrap()).unwrap();
    }
    until(|| {
        stream.poll().is_some_and(|u| {
            assert!(!matches!(u.status, Status::Failed(_)), "{:?}", u.status);
            u.status == Status::Closed
        })
    });
}

/// 取得元のウィンドウから開いたメニュー（別のウィンドウ）を、ウィンドウの外へはみ出した所も含めて取り込み、
/// メニューの項目を押すとその項目が選ばれる。
#[test]
#[ignore = "opens a real window and its menu, captures the menu and clicks an item; run on an interactive Windows desktop"]
fn context_menu_is_captured_apart_from_the_window_and_takes_clicks() {
    let _guard = NATIVE_INPUT.lock().unwrap();
    let (go, wait) = mpsc::channel::<()>();
    let (opened, ready) = mpsc::channel::<()>();
    let (chosen_tx, chosen) = mpsc::channel();
    // メニューの処理（TrackPopupMenu）はウィンドウを作ったスレッドで待つので、ウィンドウごと別のスレッドに置く。
    let owner = thread::spawn(move || unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            lpszClassName: w!("nyaw.desktop.menu.e2e"),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&class), 0);
        let window = TestWindow(Some(
            CreateWindowExW(
                WS_EX_TOPMOST,
                w!("nyaw.desktop.menu.e2e"),
                w!("nyaw desktop menu E2E"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                360,
                240,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ));
        let hwnd = window.0.unwrap();
        opened.send(()).unwrap();
        while wait.try_recv().is_err() {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            thread::sleep(Duration::from_millis(5));
        }
        let menu = CreatePopupMenu().unwrap();
        AppendMenuW(menu, MF_STRING, 1, w!("first")).unwrap();
        AppendMenuW(menu, MF_STRING, 2, w!("second")).unwrap();
        let _ = SetForegroundWindow(hwnd);
        // ウィンドウの右下寄りで開き、ウィンドウの外へはみ出させる。
        let item = TrackPopupMenu(menu, TPM_RETURNCMD, 400, 300, None, hwnd, None);
        let _ = DestroyMenu(menu);
        chosen_tx.send(item.0).unwrap();
        drop(window);
    });
    ready.recv_timeout(Duration::from_secs(15)).unwrap();
    let source = window_source("nyaw desktop menu E2E");
    let stream = Stream::start(source).unwrap();
    until(|| {
        stream.poll().is_some_and(|u| {
            assert!(!matches!(u.status, Status::Failed(_)), "{:?}", u.status);
            u.frame.is_some()
        })
    });
    assert!(stream.popups().is_empty());
    go.send(()).unwrap();
    let mut popup = None;
    until(|| {
        popup = stream.popups().first().cloned();
        popup.is_some()
    });
    let popup = popup.unwrap();
    let [x, y, w, h] = popup.area;
    assert!(
        x > 0.5 && x + w > 1.0,
        "the menu runs off the right edge: {:?}",
        popup.area
    );
    assert!(
        popup.image.rgba.chunks(4).any(|p| p[..3] != [0, 0, 0]),
        "menu pixels are captured"
    );
    // 2 つ目の項目（下寄り）を押す。
    for phase in [TouchPhase::Down, TouchPhase::Up] {
        stream
            .touch(Touch {
                id: 0,
                phase,
                x: x + w / 2.0,
                y: y + h * 0.75,
            })
            .unwrap();
    }
    let mut item = None;
    until(|| {
        item = chosen.try_recv().ok();
        item.is_some()
    });
    assert_eq!(item, Some(2));
    until(|| stream.popups().is_empty());
    owner.join().unwrap();
}

/// メニューの画像はメニューのウィンドウだけから取る。透明な所（Chrome などのメニューが周りに持つ余白）には後ろのウィンドウが
/// 写らず、メニューの透明度がそのまま（事前乗算アルファで）届く。
#[test]
#[ignore = "opens real windows and captures a translucent popup with WGC; run on an interactive Windows desktop"]
fn popup_arrives_with_its_own_alpha_and_nothing_behind_it() {
    let _guard = NATIVE_INPUT.lock().unwrap();
    const WIDTH: i32 = 200;
    const HEIGHT: i32 = 100;
    let (window, popup) = unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            lpszClassName: w!("nyaw.desktop.alpha.e2e"),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&class), 0);
        let create = |ex_style, title, style, x, y, width, height| {
            TestWindow(Some(
                CreateWindowExW(
                    ex_style,
                    w!("nyaw.desktop.alpha.e2e"),
                    title,
                    style,
                    x,
                    y,
                    width,
                    height,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            ))
        };
        let window = create(
            WS_EX_TOPMOST,
            w!("nyaw desktop alpha E2E"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            360,
            240,
        );
        let popup = create(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            w!(""),
            WS_POPUP | WS_VISIBLE,
            150,
            150,
            WIDTH,
            HEIGHT,
        );
        // 左半分は透明、右半分は不透明な赤（BGRA の u32）。
        let pixels: Vec<u32> = (0..WIDTH * HEIGHT)
            .map(|i| {
                if i % WIDTH < WIDTH / 2 {
                    0
                } else {
                    0xFFFF_0000
                }
            })
            .collect();
        paint_layered(popup.0.unwrap(), WIDTH, HEIGHT, &pixels);
        (window, popup)
    };
    let source = window_source("nyaw desktop alpha E2E");
    let stream = Stream::start(source).unwrap();
    let mut found = None;
    until(|| {
        stream.poll();
        found = stream.popups().first().cloned();
        found.is_some()
    });
    let image = found.unwrap().image;
    assert_eq!((image.width, image.height), (WIDTH as u32, HEIGHT as u32));
    let px = |x: i32| {
        let i = ((HEIGHT / 2 * WIDTH + x) * 4) as usize;
        image.rgba[i..i + 4].to_vec()
    };
    assert_eq!(
        px(WIDTH / 4),
        [0, 0, 0, 0],
        "nothing behind the popup shows"
    );
    assert_eq!(px(WIDTH * 3 / 4), [255, 0, 0, 255], "the popup itself");
    drop(popup);
    until(|| stream.popups().is_empty());
    drop(window);
}

/// レイヤードウィンドウに画素ごとの透過を持つ絵（事前乗算の BGRA）を設定する。
unsafe fn paint_layered(hwnd: HWND, width: i32, height: i32, pixels: &[u32]) {
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
        let old = SelectObject(dc, bitmap.into());
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            hwnd,
            Some(screen),
            None,
            Some(&size),
            Some(dc),
            Some(&POINT::default()),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
        .unwrap();
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
    }
}
