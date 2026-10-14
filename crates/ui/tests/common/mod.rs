//! Web のパネルの E2E の共通部分。本番と同じく GPUI のイベントループがメインスレッドを持ち、Tauri もその上で回るので、
//! テストの手順は別スレッドで流す。ページの状態は Tauri のイベント（`testing:probe` → `testing:state`）で読み、
//! GPUI 側は `ui::Hooks::remote` でフレームループに処理を差し込んで触る。

#![allow(dead_code)] // テストごとに使う部分が違う

use std::{
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, channel},
    },
    time::{Duration, Instant},
};

use kernel::vr::novr::{Button, Emulator};
use kernel::vr::{Frame, OverlayId, Quat, RawController, Vec3, mock::MockBackend};
use kernel::{Hand, Kernel, Pose, Runtime, Vec2, VrBackend, VrError};
use serde_json::Value;
use std::sync::mpsc::Sender;
use tauri::{AppHandle, Emitter, Listener};

/// どこで流すか。Linux は画面（X / Wayland）があれば毎回、ほかの OS は `--ignored` のときだけ
/// （読み戻しなど Linux でしか確かめていない経路を含むため）。
pub fn should_run(name: &str) -> bool {
    let ignored = std::env::args().any(|a| a == "--ignored" || a == "--include-ignored");
    let display =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    let run = ignored || (cfg!(target_os = "linux") && display);
    if !run {
        println!("{name}: スキップ（画面が無い。Linux 以外は --ignored で流す）");
        return false;
    }
    // ログレベルは本番と同じく NYAW_LOG（既定 info）。
    let level = std::env::var("NYAW_LOG")
        .ok()
        .and_then(|l| l.parse().ok())
        .unwrap_or(logger::Level::Info);
    let _ = logger::init(logger::Logger::new(level).sink(logger::StderrSink));
    true
}

/// テストスレッドと共有するモックのランタイム（`VrKit` と同じく OpenXR の Overlay として振る舞う）。
#[derive(Clone, Default)]
pub struct SharedMock(pub Arc<Mutex<MockBackend>>);

impl VrBackend for SharedMock {
    fn runtime(&self) -> Runtime {
        self.0.lock().unwrap().runtime()
    }
    fn poll(&mut self) -> Frame {
        // 実ランタイムの vsync 待ちの代わり（約 90Hz）。
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

/// ページ（web/src/probe.ts）への問い合わせと返事。
pub const PROBE_EVENT: &str = "testing:probe";
pub const STATE_EVENT: &str = "testing:state";

/// テストのスレッドから、フレームループ（メインスレッド）で GPUI を触る口。
pub type Remote = std::sync::mpsc::Sender<ui::Remote>;

/// 本番と同じ `ui::run_with`（GPUI のイベントループの上で Tauri も回す）で起動する。`kick` を最初のフレームで実行し、
/// Web（Tauri）が起きたら `scenario` を別スレッドで流す。最初に読むのは `label` のパネル。
/// 終わったら（失敗したらパニックの内容を出して）アプリに終了を頼み（WebView を閉じてから終わる）、終了コードを返す。
pub fn run(
    name: &'static str,
    kernel: Kernel,
    panels: Vec<ui::PanelSpec>,
    label: &'static str,
    kick: Option<ui::Remote>,
    scenario: impl FnOnce(&mut Page, &Remote) + Send + 'static,
) -> std::process::ExitCode {
    let (remote, rx) = channel::<ui::Remote>();
    if let Some(k) = kick {
        remote.send(k).unwrap();
    }
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    let hooks = ui::Hooks {
        on_web: Some(Box::new(move |app: &AppHandle| {
            let (tx, replies) = channel();
            app.listen_any(STATE_EVENT, move |e| {
                if let Ok(v) = serde_json::from_str::<Value>(e.payload()) {
                    let _ = tx.send(v);
                }
            });
            let app = app.clone();
            std::thread::spawn(move || {
                let mut page = Page {
                    app,
                    label: label.into(),
                    replies,
                    seq: 0,
                };
                let ok = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    // ページの読み込みと最初の状態を待つ。
                    page.until("ページが読み込まれ状態が届く", |s| {
                        !s["state"]["status"].is_null()
                    });
                    scenario(&mut page, &remote)
                }))
                .is_ok();
                println!("{name}: {}", if ok { "ok" } else { "FAILED" });
                *done.lock().unwrap() = Some(ok);
                quit(&remote, ok);
            });
        })),
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, webpages::FILES, hooks);
    exit_code(name, &result)
}

/// アプリに終了を頼む（フレームループが WebView を閉じてから終わる）。終わらなければ 15 秒後にプロセスごと終える。
pub fn quit(remote: &Remote, ok: bool) {
    let _ = remote.send(Box::new(|cx| {
        cx.default_global::<ui::PanelControl>().quit = true;
    }));
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        println!("終了を頼んでもアプリが終わらないので、プロセスを終える");
        logger::global().flush();
        std::process::exit(if ok { 0 } else { 1 });
    });
}

/// `run_with` が戻った後の終了コード。シナリオの結果が無ければ、終わる前にアプリが終了した。
pub fn exit_code(name: &str, result: &Mutex<Option<bool>>) -> std::process::ExitCode {
    logger::global().flush();
    match *result.lock().unwrap() {
        Some(true) => std::process::ExitCode::SUCCESS,
        Some(false) => std::process::ExitCode::FAILURE,
        None => {
            println!("{name}: FAILED（シナリオが終わる前にアプリが終了した）");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Web のページ 1 枚を読む口（`label` のパネル）。
pub struct Page {
    pub app: AppHandle,
    pub label: String,
    replies: Receiver<Value>,
    seq: u64,
}

pub const TIMEOUT: Duration = Duration::from_secs(15);

impl Page {
    /// ページの状態・要素の位置（`webtesting:state` の中身）。
    pub fn probe(&mut self) -> Value {
        self.seq += 1;
        let deadline = Instant::now() + Duration::from_secs(2);
        self.app
            .emit_to(self.label.as_str(), PROBE_EVENT, self.seq)
            .unwrap();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.replies.recv_timeout(left) {
                Ok(v) if v["seq"] == self.seq => return v,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Value::Null // まだ読み込み中など。呼び出し側は until で待つ
    }

    /// `cond` が成り立つまで問い合わせ続ける。成り立たなければ最後の状態を添えて失敗する。
    pub fn until(&mut self, what: &str, cond: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let v = self.probe();
            if !v.is_null() && cond(&v) {
                return v;
            }
            if Instant::now() > deadline {
                panic!("{what}: 時間内に成り立たない。最後の状態: {v:#}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// 要素の中心（CSS px = パネルの論理ピクセル）。
    pub fn center(&mut self, id: &str) -> (f32, f32) {
        let v = self.until(&format!("要素 {id} がある"), |v| {
            v["rects"][id].is_object()
        });
        let r = &v["rects"][id];
        let f = |k: &str| r[k].as_f64().unwrap() as f32;
        (f("x") + f("w") / 2.0, f("y") + f("h") / 2.0)
    }

    /// 要素の左端から `dx` px 内側、縦は中央（文字に掛からない所の色を見る）。
    pub fn left_edge(&mut self, id: &str, dx: f32) -> (f32, f32) {
        let v = self.until(&format!("要素 {id} がある"), |v| {
            v["rects"][id].is_object()
        });
        let r = &v["rects"][id];
        let f = |k: &str| r[k].as_f64().unwrap() as f32;
        (f("x") + dx, f("y") + f("h") / 2.0)
    }
}

/// 状態の数値。
pub fn num(v: &Value, path: &[&str]) -> f64 {
    path.iter()
        .fold(v, |v, k| &v[*k])
        .as_f64()
        .unwrap_or(f64::NAN)
}

pub const PANEL_POS: Vec3 = Vec3::new(0.0, 0.0, -1.0);
/// 1m あたりのピクセル数（800px = 0.8m）。リサイズしても変わらない。
pub const PX_PER_M: f32 = 1000.0;
pub const PROFILE: &str = "valve/index_controller";

/// 右手の仮想コントローラ。値はモックの `frame` に書き、フレームループが次の tick で読む。
pub struct Vr {
    pub mock: Arc<Mutex<MockBackend>>,
    pub aim: Pose,
    pub inputs: Vec<(String, f32)>,
    /// 操作するパネルの中心。
    pub panel: Vec3,
    /// 今のパネルの解像度（ピクセル → 空間の点の変換に使う）。
    pub resolution: (u32, u32),
}

impl Vr {
    /// 右手を下ろした位置から、正面 1m の 800×600 のパネルを指す。
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

    /// レーザーをパネル上のピクセル（CSS px）へ向ける。
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

    /// VR パネル（オーバーレイ 0）に最後に送られたテクスチャの、論理ピクセル `(x, y)` の色。
    pub fn color_at(&self, (x, y): (f32, f32)) -> Option<[u8; 4]> {
        let mock = self.mock.lock().unwrap();
        let tex = mock.overlays[0].texture.as_ref()?;
        let (rw, rh) = self.resolution;
        let tx = ((x / rw as f32 * tex.width as f32) as u32).min(tex.width - 1);
        let ty = ((y / rh as f32 * tex.height as f32) as u32).min(tex.height - 1);
        Some(tex.pixel(tx, ty))
    }

    pub fn texture_size(&self) -> Option<(u32, u32)> {
        self.texture_size_of(0)
    }

    pub fn texture_size_of(&self, overlay: usize) -> Option<(u32, u32)> {
        let mock = self.mock.lock().unwrap();
        mock.overlays[overlay]
            .texture
            .as_ref()
            .map(|t| (t.width, t.height))
    }

    /// VR パネルの `(x, y)` が `rgb` に近い色になるまで待つ。
    pub fn until_color(&self, what: &str, at: (f32, f32), rgb: [u8; 3]) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let c = self.color_at(at);
            if c.is_some_and(|[r, g, b, _]| {
                [r, g, b]
                    .iter()
                    .zip(rgb)
                    .all(|(a, b)| (*a as i32 - b as i32).abs() <= 8)
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: VR パネルの {at:?} が {rgb:02x?} にならない（{c:02x?}）"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// 要素の中心を指して、選択ボタンを押して離し、click が届くのを待つ。
pub fn click(page: &mut Page, vr: &mut Vr, id: &str) {
    press(page, vr, id);
    page.until(&format!("{id} を離した"), |s| {
        s["state"]["last"]["type"] == "click" || s["state"]["last"]["type"] == "lostpointercapture"
    });
}

/// 要素の中心を指して、選択ボタンを押して離す（離した後のページは待たない）。
pub fn press(page: &mut Page, vr: &mut Vr, id: &str) {
    vr.point_at(page.center(id));
    page.until(&format!("{id} に乗る"), |s| {
        s["state"]["last"]["target"] == id
    });
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    vr.set("trigger/value", 1.0);
    page.until(&format!("{id} を押した"), |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
    });
    vr.set("trigger/value", 0.0);
}

pub type Job = Box<dyn FnOnce(&mut Emulator) + Send>;

/// エミュレーターを持つバックエンド。テストのスレッドから送られた操作を、メインスレッドの poll で行う
/// （エミュレーターはウィンドウを持ちうるのでメインスレッドから出せない）。
pub struct Driven {
    emu: Emulator,
    jobs: Receiver<Job>,
    /// 次のフレームの始まり（ウィンドウ付きのエミュレーターと同じく、poll はここまで眠る）。
    next: Option<Instant>,
}

impl Driven {
    /// ウィンドウを出さないエミュレーターと、それを操作する口。
    pub fn headless(width: u32, height: u32) -> (Self, Emu) {
        let (tx, jobs) = channel();
        let emu = Emulator::headless(width, height);
        (
            Self {
                emu,
                jobs,
                next: None,
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
        // ウィンドウ付きのエミュレーターと同じ周期（約 90Hz）。間に合わなかったフレームは取り戻さない。
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
        // ウィンドウ付きと同じく、毎フレーム合成する（コストを本番に揃える）。
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

/// テストのスレッドからエミュレーターを操作する口。
pub struct Emu(pub Sender<Job>);

impl Emu {
    /// 次の poll で `f` を行い、結果を待つ。
    pub fn run<R: Send + 'static>(&self, f: impl FnOnce(&mut Emulator) -> R + Send + 'static) -> R {
        let (tx, rx) = channel();
        self.0
            .send(Box::new(move |e| {
                let _ = tx.send(f(e));
            }))
            .expect("フレームループが止まっている");
        rx.recv_timeout(TIMEOUT).expect("フレームループが応えない")
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

    /// 画面の `at` が `pred` を満たす色になるまで待つ。
    pub fn until_color(&self, what: &str, at: Vec2, pred: impl Fn([u8; 3]) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let c = self.color(at);
            if pred(c) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: 画面の {at:?} の色が合わない（{c:02x?}）"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
