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

use kernel::vr::{Device, OverlayId, Pose, Quat, Runtime, Vec3, VrBackend};
use testing::{BUTTON_ACTIVE_BG, Testing, TestingTransparent};
use testkit::runtimes::{self, Kind, LaunchError};
use testkit::{DesktopKit, Driver};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run_ignored = args
        .iter()
        .any(|a| a == "--ignored" || a == "--include-ignored");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let kinds = Kind::ALL
        .into_iter()
        .filter(|k| filters.is_empty() || filters.iter().any(|f| k.name().contains(f.as_str())));

    // SteamVR is launched without Wayland (runtimes::launch); the desktop scenario streams this session's display.
    let wayland = std::env::var_os("WAYLAND_DISPLAY");
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
        let t0 = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut lap = Instant::now();
            let mut mark = |what: &str| {
                eprintln!("  timing: {what} {:?}", lap.elapsed());
                lap = Instant::now();
            };
            let mut vr = rt
                .connect()
                .unwrap_or_else(|e| panic!("failed to connect: {e}"));
            mark("connect");
            overlay_reaches_runtime(vr.as_mut());
            mark("overlay");
            external_image_reaches_runtime(vr.as_mut());
            mark("external");
            if std::env::var_os("NYAW_E2E_DESKTOP").is_some() {
                desktop_reaches_runtime(vr.as_mut(), wayland.as_deref());
                mark("desktop");
            }
            fov_is_reported(vr.as_mut());
            mark("fov");
            home_runs_on_runtime(vr);
            mark("home");
            let vr = rt
                .connect()
                .unwrap_or_else(|e| panic!("failed to reconnect: {e}"));
            mark("reconnect");
            transparent_runs_on_runtime(vr);
            mark("transparent");
        }));
        let log = rt.log.clone();
        let t1 = Instant::now();
        drop(rt);
        eprintln!(
            "  timing: scenario {:?}, cleanup {:?}",
            t1 - t0,
            t1.elapsed()
        );
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

/// The runtime reports both eyes' view (after frames have run), and the HUD's safe area lies inside it, so a HUD
/// placed there (the notification toast) renders on this (simulated) headset.
fn fov_is_reported(vr: &mut dyn VrBackend) {
    let fov = vr.fov().expect("the runtime reports its field of view");
    let safe = fov.safe();
    eprintln!("  fov: {fov:?}\n  safe: {safe:?}");
    assert!(fov.valid(), "{fov:?}");
    for (s, f) in [
        (safe.left, fov.left),
        (safe.right, fov.right),
        (safe.up, fov.up),
        (safe.down, fov.down),
    ] {
        assert!(s > 0.0 && s < f, "safe {s} inside {f}");
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

/// A display server's image (shared memory, B/G/R/unused) is imported on the runtime's GPU and arrives as RGBA.
fn external_image_reaches_runtime(vr: &mut dyn VrBackend) {
    use std::sync::Arc;
    let Some(align) = vr.import_formats().and_then(|f| f.host_alignment) else {
        eprintln!("  external: this runtime cannot import shared memory");
        return;
    };
    let id = vr
        .create_overlay("nyaw.e2e.external", 0.5)
        .expect("failed to create overlay");
    vr.set_overlay_pose(id, Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY));
    let (w, h) = (64u32, 32u32);
    let len = ((w * h * 4) as usize).div_ceil(align) * align;
    let layout = std::alloc::Layout::from_size_align(len, align).unwrap();
    let ptr = unsafe { std::alloc::alloc(layout) };
    let bgrx = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    let mut want = Vec::new();
    for (i, px) in bgrx
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .take((w * h) as usize)
        .enumerate()
    {
        let (r, g, b) = ((i % 64 * 4) as u8, (i / 64 * 8) as u8, 100);
        *px = [b, g, r, 0];
        want.extend([r, g, b, 255]);
    }
    let image = Arc::new(kernel::vr::ExternalImage::new(
        w,
        h,
        kernel::vr::PixelFormat::Bgrx,
        kernel::vr::Memory::Host {
            ptr,
            len,
            offset: 0,
            stride: w * 4,
        },
        || {},
    ));
    vr.submit_overlay_external(id, &image)
        .expect("the runtime imports shared memory");
    drop(image);
    if vr.runtime() == Runtime::OpenVr {
        assert!(
            read_back(vr, id, &want) == want,
            "the imported image differs from what was sent"
        );
    }
    vr.destroy_overlay(id);
    // The runtime keeps the import until a later submission completes; send one before freeing the memory.
    let other = vr.create_overlay("nyaw.e2e.flush", 0.1).unwrap();
    vr.submit_overlay_pixels(other, 1, 1, &[0; 4]);
    for _ in 0..10 {
        vr.submit_overlay_pixels(other, 1, 1, &[0; 4]);
        vr.poll();
    }
    vr.destroy_overlay(other);
    unsafe { std::alloc::dealloc(ptr, layout) };
}

/// The desktop of this session (NYAW_E2E_DESKTOP=1): stream the first screen with the runtime's import formats
/// (DMA-BUF through the portal on Wayland), and show its frames on the runtime for a second.
fn desktop_reaches_runtime(vr: &mut dyn VrBackend, wayland: Option<&std::ffi::OsStr>) {
    use kernel::desktop::{Sources, Status, Stream};
    // SAFETY: no other thread reads the environment yet (the capture threads start below).
    if let Some(display) = wayland {
        unsafe { std::env::set_var("WAYLAND_DISPLAY", display) };
    }
    let pending = Sources::refresh().expect("list screens");
    let start = Instant::now();
    let screens = loop {
        if let Some(result) = pending.poll() {
            break result.expect("list screens");
        }
        assert!(start.elapsed() < Duration::from_secs(10), "listing screens");
        std::thread::sleep(Duration::from_millis(20));
    };
    let screen = screens.first().expect("a screen").clone();
    // NYAW_E2E_DESKTOP=rgba compares with the path that swizzles on the CPU and uploads RGBA.
    let import = vr
        .import_formats()
        .filter(|_| std::env::var("NYAW_E2E_DESKTOP").as_deref() != Ok("rgba"));
    eprintln!("  desktop: {} with {import:?}", screen.name);
    let stream = Stream::start_with(screen, 60, import).expect("start streaming");
    let _moving = animate();
    let id = vr
        .create_overlay("nyaw.e2e.desktop", 1.0)
        .expect("failed to create overlay");
    vr.set_overlay_pose(id, Pose::new(Vec3::new(0.0, 1.5, -1.5), Quat::IDENTITY));
    let (mut frames, mut external, mut size) = (0, 0, (0, 0));
    let (mut cost, mut latency) = (Duration::ZERO, Duration::ZERO);
    let start = Instant::now();
    // A still screen sends few frames (the compositor sends on change): stop after 2 s once one has arrived.
    while start.elapsed() < Duration::from_secs(30)
        && !(frames > 0 && start.elapsed() > Duration::from_secs(2))
    {
        if let Some(update) = stream.poll() {
            if let Status::Failed(e) = &update.status {
                panic!("desktop capture failed: {e}");
            }
            if let Some(frame) = update.frame {
                // From the display server's frame (as the capture worker received it) to the runtime.
                let submitted = Instant::now();
                latency += submitted.saturating_duration_since(frame.captured) + frame.cost;
                match &frame.external {
                    Some(image) => {
                        vr.submit_overlay_external(id, image)
                            .expect("import a desktop frame");
                        external += 1;
                    }
                    None => vr.submit_overlay_pixels(id, frame.width, frame.height, &frame.rgba),
                }
                cost += submitted.elapsed();
                if frames == 0 {
                    eprintln!("  desktop: first frame {:?}", frame.external);
                }
                size = (frame.width, frame.height);
                frames += 1;
            }
        }
        vr.poll();
    }
    eprintln!(
        "  desktop: {frames} frames ({external} imported) of {size:?}, submitted in {:?} on average, {:?} after capture",
        cost / frames.max(1),
        (latency + cost) / frames.max(1),
    );
    assert!(frames > 0, "no desktop frame arrived");
    if vr.runtime() == Runtime::OpenVr {
        let (w, h, rgba) = kernel::vr::openvr_overlay_image(id).expect("read back the desktop");
        assert_eq!((w, h), size);
        assert!(
            rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
            "the desktop is opaque"
        );
        assert!(rgba.iter().any(|&v| v != 0), "the desktop is not black");
    }
    drop(stream);
    vr.destroy_overlay(id);
    if wayland.is_some() {
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
}

/// Keeps repainting a strip along the top of the X server's whole screen (XWayland on Wayland, so it crosses every
/// monitor) until dropped, so the compositor keeps sending frames while the desktop scenario measures them.
#[cfg(target_os = "linux")]
fn animate() -> Option<impl Drop> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use x11rb::{connection::Connection, protocol::xproto::*};
    struct Stop(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
            if let Some(t) = self.1.take() {
                let _ = t.join();
            }
        }
    }
    let (conn, screen) = x11rb::connect(None).ok()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let thread = std::thread::spawn(move || {
        let root = conn.setup().roots[screen].root;
        let width = conn.setup().roots[screen].width_in_pixels;
        let window = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            width,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().override_redirect(1),
        )
        .unwrap();
        conn.map_window(window).unwrap();
        let mut i = 0u32;
        while !stopping.load(Ordering::Acquire) {
            i = i.wrapping_add(1);
            let color = (i * 7919) & 0xff_ffff;
            let _ = conn.change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new().background_pixel(color),
            );
            let _ = conn.clear_area(false, window, 0, 0, 0, 0);
            let _ = conn.flush();
            std::thread::sleep(Duration::from_millis(16));
        }
    });
    Some(Stop(stop, Some(thread)))
}

#[cfg(not(target_os = "linux"))]
fn animate() -> Option<()> {
    None
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
        if let Ok((w, h, rgba)) = kernel::vr::openvr_overlay_image(id) {
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
            Runtime::OpenVr => kernel::vr::openvr_overlay_image(id).ok(),
            _ => kit.kernel.subsystems.backend.overlay_image(id),
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
        if let Ok((_, _, rgba)) = kernel::vr::openvr_overlay_image(id) {
            got = rgba;
            if got == want {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    got
}
