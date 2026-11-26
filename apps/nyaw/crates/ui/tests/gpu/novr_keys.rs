//! E2E for the novr emulator's keyboard on the production `ui::run_with`: every X key press posted to the emulator's
//! window reaches the kernel, so each double press is counted. GPUI's message loop shares the thread with the
//! emulator's window and used to drop its WM_KEYDOWN (`translate_accelerator` in vendor/gpui_windows), so double
//! presses were counted only by luck. Opens a real window, so it only runs with `--ignored` (Windows only).

use crate::common;

use std::process::ExitCode;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, Runtime};
use testing::Testing;
use ui::PanelSpec;

/// The emulator window's title (`vr::novr`).
const TITLE: &str = "nyaw (novr)";
/// Double presses to send.
const PRESSES: u32 = 40;
/// Time between the key events of one double press (down, up, down, up).
const STEP: Duration = Duration::from_millis(30);

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
}

pub fn main() -> ExitCode {
    if !cfg!(windows) || !common::should_run("novr_keys") {
        return ExitCode::SUCCESS;
    }
    // the windowed emulator, as `--novr` in production.
    let backend = kernel::vr::connect(Some(Runtime::NoVr), true).expect("novr is always there");
    let kernel = Kernel::with_backend(backend, Default::default());
    let panels = vec![
        PanelSpec::root(
            Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY),
            |id, _, _| Testing::new(id),
        )
        .without_nav_bar(),
    ];
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&remote)));
        let ok = ok.is_ok();
        println!("novr_keys: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, hooks);
    common::exit_code("novr_keys", &result)
}

/// The double presses the kernel has counted so far.
fn double_presses(remote: &Sender<ui::Remote>) -> u32 {
    let (tx, rx) = channel();
    remote
        .send(Box::new(move |cx| {
            let ctl = cx.default_global::<ui::PanelControl>();
            let _ = tx.send(ctl.panels.get(&0).map_or(0, |p| p.status.double_press));
        }))
        .expect("frame loop has stopped");
    rx.recv_timeout(common::TIMEOUT)
        .expect("frame loop is not responding")
}

fn scenario(remote: &Sender<ui::Remote>) {
    let title: Vec<u16> = TITLE.encode_utf16().chain([0]).collect();
    let deadline = Instant::now() + common::TIMEOUT;
    let hwnd = loop {
        let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if hwnd != 0 {
            break hwnd;
        }
        assert!(
            Instant::now() < deadline,
            "the emulator window never opened"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    // let the first frames (and the panel's first render) pass.
    std::thread::sleep(Duration::from_secs(1));
    // the X key as the OS posts it: WM_KEYDOWN / WM_KEYUP with its scan code (0x2D) in the lparam.
    let key = |down: bool| {
        let (msg, lparam) = if down {
            (0x0100, 0x002D_0001)
        } else {
            (0x0101, 0xC02D_0001_u32 as i32 as isize)
        };
        assert_ne!(unsafe { PostMessageW(hwnd, msg, 0x58, lparam) }, 0);
        std::thread::sleep(STEP);
    };
    for _ in 0..PRESSES {
        for down in [true, false, true, false] {
            key(down);
        }
        // longer than `kernel::DOUBLE_PRESS`, so the next press starts a new pair.
        std::thread::sleep(kernel::DOUBLE_PRESS + Duration::from_millis(50));
    }
    let counted = double_presses(remote);
    println!("novr_keys: {counted} of {PRESSES} double presses counted");
    assert_eq!(counted, PRESSES, "every double press reaches the kernel");
}
