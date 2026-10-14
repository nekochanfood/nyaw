//! E2E on real runtimes: launch the runtime -> connect -> send a different image to the overlay every frame ->
//! operate the Testing screen and send its rendering to the real runtime's overlay ->
//! (OpenVR only) read back from the runtime to check it arrived -> check that the processes are gone after cleanup.
//! Real runtimes have no controllers (null driver / simulated), so Testing is operated with the mouse.
//!
//! Startup takes several seconds and shows on screen, so like `#[ignore]` it runs only when `--ignored` is passed.
//! Runtimes that are not installed are skipped (install them with `cargo runtimes`).
//!
//! ```text
//! cargo test -p testkit --test runtimes -- --ignored           all
//! cargo test -p testkit --test runtimes -- --ignored monado    filter by name
//! ```

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use testing::{BUTTON_ACTIVE_BG, Testing, TestingTransparent};
use testkit::runtimes::{self, Kind, LaunchError};
use testkit::{DesktopKit, Driver};
use vr::{Device, OverlayId, Pose, Quat, Runtime, Vec3, VrBackend};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run_ignored = args
        .iter()
        .any(|a| a == "--ignored" || a == "--include-ignored");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let kinds = Kind::ALL
        .into_iter()
        .filter(|k| filters.is_empty() || filters.iter().any(|f| k.name().contains(f.as_str())));

    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    for kind in kinds {
        print!("test runtimes::{kind} ... ");
        if !run_ignored {
            println!("ignored (run with --ignored)");
            skipped += 1;
            continue;
        }
        // SAFETY: the main of harness = false is single-threaded
        let mut rt = match unsafe { runtimes::launch(kind) } {
            Ok(rt) => rt,
            Err(e @ (LaunchError::NotInstalled(_) | LaunchError::AlreadyRunning(_))) => {
                println!("skipped（{e}）");
                skipped += 1;
                continue;
            }
            Err(e) => {
                println!("FAILED\n  failed to launch: {e}");
                failed += 1;
                continue;
            }
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut vr = rt
                .connect()
                .unwrap_or_else(|e| panic!("failed to connect: {e}"));
            overlay_reaches_runtime(vr.as_mut());
            home_runs_on_runtime(vr);
            let vr = rt
                .connect()
                .unwrap_or_else(|e| panic!("failed to reconnect: {e}"));
            transparent_runs_on_runtime(vr);
        }));
        let log = rt.log.clone();
        drop(rt);
        let leftover = kind.is_running();
        match result {
            Ok(()) if !leftover => {
                println!("ok");
                passed += 1;
            }
            Ok(()) => {
                println!("FAILED\n  {kind} processes remain after cleanup");
                failed += 1;
            }
            Err(_) => {
                println!("FAILED (log: {log:?})");
                failed += 1;
            }
        }
    }
    println!("\ntest result: {passed} passed; {failed} failed; {skipped} skipped");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn overlay_reaches_runtime(vr: &mut dyn VrBackend) {
    let id = vr
        .create_overlay("nyaw.e2e", 0.5)
        .expect("failed to create overlay");
    vr.set_overlay_pose(id, Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY));

    // Sending a different image every frame at the real panel size (1024x768) at 90Hz for 1 second must not stall.
    let (w, h) = (1024, 768);
    let start = Instant::now();
    let mut frames = 0u32;
    while start.elapsed() < Duration::from_secs(1) {
        let v = (frames % 256) as u8;
        vr.submit_overlay_pixels(id, w, h, &[v, 255 - v, 0, 255].repeat((w * h) as usize));
        vr.poll();
        frames += 1;
        std::thread::sleep(Duration::from_millis(11));
    }
    assert!(frames > 30, "only {frames} frames sent in 1 second");

    // The last image sent must have reached the runtime. OpenXR has no way to read back.
    if vr.runtime() == Runtime::OpenVr {
        for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            let rgba = color.repeat(64 * 64);
            vr.submit_overlay_pixels(id, 64, 64, &rgba);
            assert!(
                read_back(vr, id, &rgba) == rgba,
                "the runtime image differs from what was sent ({color:?})"
            );
        }
    }
}

/// Operates the Testing screen on a real runtime: attaches the panel to the HMD with a button and locks it,
/// and the result (the lit button) must reach the runtime's overlay (only OpenVR can be read back).
fn home_runs_on_runtime(vr: Box<dyn VrBackend>) {
    let runtime = vr.runtime();
    let mut cx = gpui::TestAppContext::single();
    let mut kit = DesktopKit::with_backend(&mut cx, vr, |_, _| Testing::default());
    kit.click("attach-hmd").click("lock").hover("click-me");
    let p = kit.kernel.panel(kit.panel);
    assert_eq!(
        (p.attached.as_ref().map(|a| &a.0), p.locked),
        (Some(&Device::Hmd), true),
        "Testing buttons have no effect on the panel"
    );
    // While running the real runtime's frames, keep sending rendering to the panel attached to the HMD.
    for _ in 0..30 {
        kit.step();
        std::thread::sleep(Duration::from_millis(11));
    }
    if runtime != Runtime::OpenVr {
        return;
    }
    let c = kit.bounds("lock").center();
    let (res, id) = {
        let p = kit.kernel.panel(kit.panel);
        (p.resolution, p.overlay_id())
    };
    let start = Instant::now();
    let mut got = None;
    while start.elapsed() < Duration::from_secs(5) {
        kit.step();
        if let Ok((w, h, rgba)) = vr::openvr_overlay_image(id) {
            let x = (f32::from(c.x) / res.0 as f32 * w as f32) as u32;
            let y = (f32::from(c.y) / res.1 as f32 * h as f32) as u32;
            let i = ((y * w + x) * 4) as usize;
            got = Some(u32::from_be_bytes([0, rgba[i], rgba[i + 1], rgba[i + 2]]));
            if got == Some(BUTTON_ACTIVE_BG) {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the lock button is not lit on the runtime panel ({got:06x?})");
}

/// Sends the semi-transparent screen (testing-transparent) to a real runtime; the background must arrive still semi-transparent.
/// For OpenVR, reads back the runtime-side image; for OpenXR, the swapchain image passed in, from the GPU,
/// and checks that the background alpha is neither 0 nor 255 (OpenXR compositor output cannot be read).
fn transparent_runs_on_runtime(vr: Box<dyn VrBackend>) {
    let runtime = vr.runtime();
    let mut cx = gpui::TestAppContext::single();
    let mut kit = DesktopKit::with_backend(&mut cx, vr, |_, _| TestingTransparent::new(0));
    let id = kit.kernel.panel(kit.panel).overlay_id();
    let start = Instant::now();
    let mut px = None;
    while start.elapsed() < Duration::from_secs(5) {
        kit.step();
        let image = match runtime {
            Runtime::OpenVr => vr::openvr_overlay_image(id).ok(),
            _ => kit.kernel.vr.overlay_image(id),
        };
        if let Some((_, _, rgba)) = image {
            px = Some([rgba[0], rgba[1], rgba[2], rgba[3]]);
            // With premultiplication RGB would shrink to 0x20 * 160 / 255. It must arrive as straight alpha (the reason for setting UNPREMULTIPLIED_ALPHA on the layer).
            if matches!(rgba[3], 1..=254) {
                assert_eq!(rgba[..3], [0x20; 3], "should be straight alpha");
                eprintln!("  {runtime:?} background pixel = {px:?}");
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("did not reach the runtime semi-transparent (background pixel = {px:?})");
}

/// The compositor ingests asynchronously, so wait a little until it arrives. If it never does, returns the last value read.
fn read_back(vr: &mut dyn VrBackend, id: OverlayId, want: &[u8]) -> Vec<u8> {
    let start = Instant::now();
    let mut got = vec![];
    while start.elapsed() < Duration::from_secs(5) {
        vr.poll();
        if let Ok((_, _, rgba)) = vr::openvr_overlay_image(id) {
            got = rgba;
            if got == want {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    got
}
