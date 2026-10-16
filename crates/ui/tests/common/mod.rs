//! 本番の GPUI フレームループで動かす E2E の共有処理。

#![allow(dead_code)] // each test uses a different subset

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, channel},
    },
    time::{Duration, Instant},
};

use kernel::vr::novr::{Button, Emulator};
use kernel::vr::{Frame, OverlayId, Quat, RawController, Vec3, mock::MockBackend};
use kernel::{Hand, Pose, Runtime, Vec2, VrBackend, VrError};
use std::sync::mpsc::Sender;

/// where to run. On Linux every time (GPUI runs headless and draws offscreen, so no display is needed; a software
/// GPU such as Mesa's lavapipe is enough), on other OSes only with `--ignored` (they open real hidden windows).
pub fn should_run(name: &str) -> bool {
    let ignored = std::env::args().any(|a| a == "--ignored" || a == "--include-ignored");
    let run = ignored || cfg!(target_os = "linux");
    if !run {
        println!("{name}: skipped (run with --ignored on non-Linux)");
        return false;
    }
    // the log level is NYAW_LOG as in production (default info).
    let level = std::env::var("NYAW_LOG")
        .ok()
        .and_then(|l| l.parse().ok())
        .unwrap_or(logger::Level::Info);
    let _ = logger::init(logger::Logger::new(level).sink(logger::StderrSink));
    true
}

/// mock runtime shared with the test thread (behaves as an OpenXR Overlay like `VrKit`).
#[derive(Clone, Default)]
pub struct SharedMock(pub Arc<Mutex<MockBackend>>);

impl VrBackend for SharedMock {
    fn runtime(&self) -> Runtime {
        self.0.lock().unwrap().runtime()
    }
    fn poll(&mut self) -> Frame {
        // stands in for the real runtime's vsync wait (about 90Hz).
        std::thread::sleep(Duration::from_millis(11));
        self.0.lock().unwrap().poll()
    }
    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        self.0.lock().unwrap().create_overlay(key, width_m)
    }
    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.0.lock().unwrap().set_overlay_pose(id, pose)
    }
    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        self.0.lock().unwrap().set_overlay_width(id, width_m)
    }
    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        self.0.lock().unwrap().set_overlay_height(id, height_m)
    }
    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        self.0.lock().unwrap().set_overlay_visible(id, visible)
    }
    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        self.0.lock().unwrap().set_overlay_alpha(id, alpha)
    }
    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        self.0.lock().unwrap().set_overlay_sort(id, order)
    }
    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .submit_overlay_pixels(id, width, height, rgba)
    }
}

/// 終了を頼み、15 秒経っても終わらなければプロセスを終了する。
pub fn quit(remote: &Sender<ui::Remote>, ok: bool) {
    let _ = remote.send(Box::new(|cx| {
        cx.default_global::<ui::PanelControl>().quit = true;
    }));
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        println!("the app did not quit when asked, so ending the process");
        logger::global().flush();
        std::process::exit(if ok { 0 } else { 1 });
    });
}

/// exit code after `run_with` returns. If there is no scenario result, the app exited before it finished.
pub fn exit_code(name: &str, result: &Mutex<Option<bool>>) -> std::process::ExitCode {
    logger::global().flush();
    match *result.lock().unwrap() {
        Some(true) => std::process::ExitCode::SUCCESS,
        Some(false) => std::process::ExitCode::FAILURE,
        None => {
            println!("{name}: FAILED (the app exited before the scenario finished)");
            std::process::ExitCode::FAILURE
        }
    }
}

pub const TIMEOUT: Duration = Duration::from_secs(15);

pub const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// pixels per meter (800px = 0.8m). Unchanged by resizing.
pub const PX_PER_M: f32 = 1000.0;
pub const PROFILE: &str = "valve/index_controller";

/// the right-hand virtual controller. Values are written to the mock's `frame` and read by the frame loop on the next tick.
pub struct Vr {
    pub mock: Arc<Mutex<MockBackend>>,
    pub aim: Pose,
    pub inputs: Vec<(String, f32)>,
    /// center of the panel being operated.
    pub panel: Vec3,
    /// current panel resolution (used to convert pixels to space points).
    pub resolution: (u32, u32),
}

impl Vr {
    /// from the lowered right hand's position, point at an 800x600 panel 1m straight ahead.
    pub fn new(mock: SharedMock) -> Self {
        Self {
            mock: mock.0,
            aim: Pose::new(Vec3::new(0.2, -0.3, 0.0), Quat::IDENTITY),
            inputs: Vec::new(),
            panel: PANEL_POS,
            resolution: (800, 600),
        }
    }

    pub fn send(&self) {
        self.mock.lock().unwrap().frame.controllers = vec![RawController {
            hand: Hand::Right,
            profile: PROFILE.into(),
            aim: self.aim,
            inputs: self.inputs.clone(),
        }];
    }

    /// パネルの論理ピクセルへレーザーを向ける。
    pub fn point_at(&mut self, (x, y): (f32, f32)) {
        let (w, h) = (self.resolution.0 as f32, self.resolution.1 as f32);
        let target =
            self.panel + Vec3::new((x - w / 2.0) / PX_PER_M, (h / 2.0 - y) / PX_PER_M, 0.0);
        let dir = (target - self.aim.position).normalize();
        self.aim.orientation = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
        self.send();
    }

    pub fn set(&mut self, input: &str, value: f32) {
        self.inputs.retain(|(n, _)| n != input);
        if value != 0.0 {
            self.inputs.push((input.into(), value));
        }
        self.send();
    }

    pub fn texture_size_of(&self, overlay: usize) -> Option<(u32, u32)> {
        let mock = self.mock.lock().unwrap();
        mock.overlays
            .get(overlay)?
            .texture
            .as_ref()
            .map(|t| (t.width, t.height))
    }
}

pub type Job = Box<dyn FnOnce(&mut Emulator) + Send>;

/// a backend that owns the emulator. Operations sent from the test thread run in the main thread's poll
/// (the emulator may own a window, so it cannot leave the main thread).
pub struct Driven {
    emu: Emulator,
    jobs: Receiver<Job>,
    /// start of the next frame (as with a windowed emulator, poll sleeps until here).
    next: Option<Instant>,
    /// the emulator shows a window, makes its own cycle, and composites ([`Driven::scripted_window`]).
    windowed: bool,
}

impl Driven {
    /// an emulator without a window, and a way to drive it.
    pub fn headless(width: u32, height: u32) -> (Self, Emu) {
        let (tx, jobs) = channel();
        let emu = Emulator::headless(width, height);
        (
            Self {
                emu,
                jobs,
                next: None,
                windowed: false,
            },
            Emu(tx),
        )
    }

    /// an emulator that shows a window and composites and displays as in production (on GPU if available), and a way to drive it.
    /// input is injected through `Emu`, not from the window.
    pub fn scripted_window(width: u32, height: u32) -> (Self, Emu) {
        let (tx, jobs) = channel();
        let emu = Emulator::scripted_window(width, height);
        (
            Self {
                emu,
                jobs,
                next: None,
                windowed: true,
            },
            Emu(tx),
        )
    }
}

impl VrBackend for Driven {
    fn runtime(&self) -> Runtime {
        self.emu.runtime()
    }
    fn poll(&mut self) -> Frame {
        if self.windowed {
            // the emulator handles the cycle, compositing, and display.
            while let Ok(job) = self.jobs.try_recv() {
                job(&mut self.emu);
            }
            return self.emu.poll();
        }
        // same cycle as a windowed emulator (about 90Hz). Missed frames are not made up.
        let frame = Duration::from_nanos(1_000_000_000 / 90);
        let now = Instant::now();
        let next = self.next.unwrap_or(now);
        if next > now {
            std::thread::sleep(next - now);
        }
        self.next = Some(next.max(Instant::now() - frame) + frame);
        while let Ok(job) = self.jobs.try_recv() {
            job(&mut self.emu);
        }
        let frame = self.emu.poll();
        // as with the windowed one, composite every frame (to match production cost).
        self.emu.composite();
        frame
    }
    fn compositor_time(&self) -> Duration {
        self.emu.compositor_time()
    }
    fn frame_interval(&self) -> Option<Duration> {
        self.emu.frame_interval()
    }
    fn overlay_seen(&self, id: OverlayId) -> Option<bool> {
        self.emu.overlay_seen(id)
    }
    fn create_overlay(&mut self, key: &str, width_m: f32) -> Result<OverlayId, VrError> {
        self.emu.create_overlay(key, width_m)
    }
    fn set_overlay_pose(&mut self, id: OverlayId, pose: Pose) {
        self.emu.set_overlay_pose(id, pose)
    }
    fn set_overlay_width(&mut self, id: OverlayId, width_m: f32) {
        self.emu.set_overlay_width(id, width_m)
    }
    fn set_overlay_height(&mut self, id: OverlayId, height_m: f32) {
        self.emu.set_overlay_height(id, height_m)
    }
    fn set_overlay_visible(&mut self, id: OverlayId, visible: bool) {
        self.emu.set_overlay_visible(id, visible)
    }
    fn set_overlay_alpha(&mut self, id: OverlayId, alpha: f32) -> bool {
        self.emu.set_overlay_alpha(id, alpha)
    }
    fn set_overlay_sort(&mut self, id: OverlayId, order: u32) {
        self.emu.set_overlay_sort(id, order)
    }
    fn submit_overlay_pixels(&mut self, id: OverlayId, width: u32, height: u32, rgba: &[u8]) {
        self.emu.submit_overlay_pixels(id, width, height, rgba)
    }
}

/// a way for the test thread to drive the emulator.
pub struct Emu(pub Sender<Job>);

impl Emu {
    /// run `f` on the next poll and wait for the result.
    pub fn run<R: Send + 'static>(&self, f: impl FnOnce(&mut Emulator) -> R + Send + 'static) -> R {
        let (tx, rx) = channel();
        self.0
            .send(Box::new(move |e| {
                let _ = tx.send(f(e));
            }))
            .expect("frame loop has stopped");
        rx.recv_timeout(TIMEOUT)
            .expect("frame loop is not responding")
    }

    pub fn point(&self, at: Option<Vec2>) {
        self.run(move |e| e.point(at));
    }

    pub fn button(&self, button: Button, down: bool) {
        self.run(move |e| e.set_button(button, down));
    }

    pub fn stick(&self, value: Vec2) {
        self.run(move |e| e.set_stick(Hand::Right, value));
    }

    pub fn color(&self, at: Vec2) -> [u8; 3] {
        let (w, _, rgba) = self.run(|e| e.screenshot());
        let i = ((at.y as u32 * w + at.x as u32) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    }

    /// wait until the screen color at `at` satisfies `pred`.
    pub fn until_color(&self, what: &str, at: Vec2, pred: impl Fn([u8; 3]) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let c = self.color(at);
            if pred(c) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: screen color at {at:?} does not match ({c:02x?})"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
