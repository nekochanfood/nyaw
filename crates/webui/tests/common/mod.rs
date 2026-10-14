//! WebView の E2E の共通部分。Tauri はメインスレッドでイベントループを回すので、テストの手順は別スレッドで流し、
//! ページ（webtesting）の状態は Tauri のイベント（`webtesting:probe` → `webtesting:state`）で読む。

#![allow(dead_code)] // テストごとに使う部分が違う

use std::{
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, channel},
    },
    time::{Duration, Instant},
};

use kernel::vr::{Frame, OverlayId, Quat, RawController, Vec3, mock::MockBackend};
use kernel::{Hand, Kernel, Pose, Runtime, Surface, VrBackend, VrError};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Listener};
use webui::WebPanelSpec;

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
    fn surface(&self) -> Surface {
        self.0.lock().unwrap().surface()
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

/// `kernel` と `panels` で Web UI を起動し、`scenario` を別スレッドで流す。失敗（パニック）したら終了コードで返す。
pub fn run<B: VrBackend + 'static>(
    name: &'static str,
    kernel: Kernel<B>,
    panels: Vec<WebPanelSpec>,
    scenario: impl FnOnce(&mut Page) + Send + 'static,
) -> std::process::ExitCode {
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    let code = webui::run_with(kernel, panels, move |app| {
        let (tx, rx) = channel();
        app.listen_any(webtesting::STATE_EVENT, move |e| {
            if let Ok(v) = serde_json::from_str::<Value>(e.payload()) {
                let _ = tx.send(v);
            }
        });
        let app = app.clone();
        std::thread::spawn(move || {
            let mut page = Page {
                app: app.clone(),
                label: "panel-0".into(),
                replies: rx,
                seq: 0,
            };
            // ページの読み込みと最初の状態を待つ。
            page.until("ページが読み込まれ状態が届く", |s| {
                !s["state"]["status"].is_null()
            });
            let ok = std::panic::catch_unwind(AssertUnwindSafe(|| scenario(&mut page))).is_ok();
            *done.lock().unwrap() = Some(ok);
            app.exit(if ok { 0 } else { 1 });
        });
    });
    match *result.lock().unwrap() {
        Some(true) if code == 0 => {
            println!("{name}: ok");
            std::process::ExitCode::SUCCESS
        }
        r => {
            println!("{name}: FAILED (結果 {r:?}, 終了コード {code})");
            std::process::ExitCode::FAILURE
        }
    }
}

/// webtesting のページ 1 枚を読む口。
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
            .emit_to(self.label.as_str(), webtesting::PROBE_EVENT, self.seq)
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
