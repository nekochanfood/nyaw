//! 実デスクトップを使用するため、明示的に --ignored を指定したときだけ実行する。
#![cfg(windows)]

use desktop::{SourceKind, Sources, Status, Stream, Touch, TouchPhase};
use std::{
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{GetStockObject, HBRUSH, WHITE_BRUSH},
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
        .find(|s| s.name == "nyaw desktop visibility source")
        .unwrap();
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
