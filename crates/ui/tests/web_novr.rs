//! novr（エミュレーター）の E2E。本番と同じ `ui::run_with` で GPUI の Testing を開き、そこから Testing の Web 版を開く
//! （ボタンと同じく `PanelControl::spawns` に `PanelSpec::spawn_web` を積む。ボタン自体は testkit の `open_web_testing`）。
//! 本番の novr と同じく WebView は画面に出さずに描いて読み戻し、エミュレーターの画面に合成される。入力はエミュレーターの
//! マウス（右手のコントローラ）: 左クリック = トリガー、右クリック = グリップ、矢印キー = スティック。ページへはブリッジの合成イベントで届く。
//! エミュレーターはウィンドウを出さない（headless）で、テストのスレッドから [`Emu`] で操作する。
//! Linux は画面（Xvfb でよい）があれば `cargo test -p ui --test web_novr` で流れる。

mod common;

use std::process::ExitCode;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use common::{Page, num};
use kernel::vr::novr::{Button, Emulator};
use kernel::vr::{Frame, OverlayId, Quat, Vec3};
use kernel::{Hand, Kernel, Pose, Runtime, Vec2, VrBackend, VrError};
use serde_json::Value;
use ui::{PanelControl, PanelSpec};

/// GPUI の Testing（パネル 0）の位置。Web 版（パネル 1）はその右 1.2m に開く。
const GPUI_POS: Vec3 = Vec3::new(-0.6, 0.0, -1.5);
const WEB_POS: Vec3 = Vec3::new(0.6, 0.0, -1.5);

type Job = Box<dyn FnOnce(&mut Emulator) + Send>;

/// エミュレーターを持つバックエンド。テストのスレッドから送られた操作を、メインスレッドの poll で行う
/// （エミュレーターはウィンドウを持ちうるのでメインスレッドから出せない）。
struct Driven {
    emu: Emulator,
    jobs: Receiver<Job>,
}

impl VrBackend for Driven {
    fn runtime(&self) -> Runtime {
        self.emu.runtime()
    }
    fn poll(&mut self) -> Frame {
        // ウィンドウ付きのエミュレーターが poll で待つ周期の代わり（約 90Hz）。
        std::thread::sleep(Duration::from_millis(11));
        while let Ok(job) = self.jobs.try_recv() {
            job(&mut self.emu);
        }
        self.emu.poll()
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
struct Emu(Sender<Job>);

impl Emu {
    /// 次の poll で `f` を行い、結果を待つ。
    fn run<R: Send + 'static>(&self, f: impl FnOnce(&mut Emulator) -> R + Send + 'static) -> R {
        let (tx, rx) = channel();
        self.0
            .send(Box::new(move |e| {
                let _ = tx.send(f(e));
            }))
            .expect("フレームループが止まっている");
        rx.recv_timeout(common::TIMEOUT)
            .expect("フレームループが応えない")
    }

    fn point(&self, at: Option<Vec2>) {
        self.run(move |e| e.point(at));
    }

    fn button(&self, button: Button, down: bool) {
        self.run(move |e| e.set_button(button, down));
    }

    fn stick(&self, value: Vec2) {
        self.run(move |e| e.set_stick(Hand::Right, value));
    }

    fn color(&self, at: Vec2) -> [u8; 3] {
        let (w, _, rgba) = self.run(|e| e.screenshot());
        let i = ((at.y as u32 * w + at.x as u32) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    }

    /// 画面の `at` が `pred` を満たす色になるまで待つ。
    fn until_color(&self, what: &str, at: Vec2, pred: impl Fn([u8; 3]) -> bool) {
        let deadline = Instant::now() + common::TIMEOUT;
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

    /// Web のパネル上の点（CSS px）が映る画面上の点。大きさ・解像度はページが受け取った状態から読む。
    fn on_screen(&self, status: &Value, (x, y): (f32, f32)) -> Vec2 {
        let f = |v: &Value| v.as_f64().unwrap() as f32;
        let (sw, sh) = (f(&status["size"][0]), f(&status["size"][1]));
        let (rw, rh) = (f(&status["resolution"][0]), f(&status["resolution"][1]));
        let p = WEB_POS + Vec3::new((x / rw - 0.5) * sw, (0.5 - y / rh) * sh, 0.0);
        self.run(move |e| e.project(p)).expect("パネルは目の前")
    }
}

fn near(rgb: [u8; 3]) -> impl Fn([u8; 3]) -> bool {
    move |c| {
        c.iter()
            .zip(rgb)
            .all(|(a, b)| (*a as i32 - b as i32).abs() <= 8)
    }
}

fn gray(c: [u8; 3]) -> bool {
    c[0] == c[1] && c[1] == c[2]
}

fn main() -> ExitCode {
    if !common::should_run("web_novr") {
        return ExitCode::SUCCESS;
    }
    let (tx, jobs) = channel();
    let emu = Driven {
        emu: Emulator::headless(1280, 720),
        jobs,
    };
    // 手ブレ抑えは指した瞬間に当たらなくなるので切る（testkit と同じ）。
    let config = kernel::config::Config {
        pointer_smoothing: Some(0.0),
        ..Default::default()
    };
    let kernel = Kernel::with_backend(Box::new(emu) as Box<dyn VrBackend>, config);
    // GPUI の Testing（パネル 0）から、Web 版（パネル 1）を開く。
    let gpui = PanelSpec::new(Pose::new(GPUI_POS, Quat::IDENTITY), |id, _, _| {
        testing::Testing::new(id)
    });
    let open_web: ui::Remote = Box::new(|cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn_web(0, testing::WEB_PAGE))
    });
    common::run(
        "web_novr",
        kernel,
        vec![gpui],
        "panel-1",
        Some(open_web),
        move |page, _| scenario(page, &Emu(tx)),
    )
}

/// 要素の中心をマウスで指す。
fn hover(page: &mut Page, emu: &Emu, id: &str) -> Vec2 {
    let status = page.probe()["state"]["status"].clone();
    let at = emu.on_screen(&status, page.center(id));
    emu.point(Some(at));
    page.until(&format!("{id} に乗る"), |s| {
        s["state"]["last"]["target"] == id
    });
    at
}

/// 要素の中心を左クリック（トリガー）する。
fn click(page: &mut Page, emu: &Emu, id: &str) {
    hover(page, emu, id);
    let downs = num(&page.probe(), &["state", "counts", "pointerdown"]);
    emu.button(Button::Trigger, true);
    page.until(&format!("{id} を押した"), |s| {
        num(s, &["state", "counts", "pointerdown"]) > downs
    });
    emu.button(Button::Trigger, false);
}

fn scenario(page: &mut Page, emu: &Emu) {
    let s = page.probe();
    assert_eq!(s["state"]["status"]["panel"], 1);
    assert_eq!(s["state"]["status"]["runtime"], "novr");
    let status = s["state"]["status"].clone();
    let h = status["resolution"][1].as_f64().expect("解像度") as f32;

    // 画面に出さない WebView が読み戻されて、エミュレーターの画面に映る（背景は不透明な #202020）。
    // パネルの外（2 枚の間）は無彩色の市松模様。
    let bottom_left = emu.on_screen(&status, (4.0, h - 10.0));
    emu.until_color(
        "ページの描画が画面に映る",
        bottom_left,
        near([0x20, 0x20, 0x20]),
    );
    let between = emu.run(|e| e.project(Vec3::new(0.0, 0.0, -1.5))).unwrap();
    assert!(gray(emu.color(between)), "パネルの外は無彩色");
    assert_ne!(
        emu.color(Vec2::new(0.0, 0.0)),
        emu.color(Vec2::new(16.0, 0.0)),
        "市松模様"
    );

    // ホバー: マウスの方向が右手のレイになり、ブリッジの合成イベントで届く（Tailwind の hover: の色まで映る）。
    hover(page, emu, "click-me");
    let s = page.until("マウスが乗る", |s| s["state"]["hovered"] == true);
    assert_eq!(
        s["state"]["lastPointer"]["trusted"], false,
        "ブリッジが合成したイベント"
    );
    let edge = emu.on_screen(&status, page.left_edge("click-me", 3.0));
    emu.until_color("ホバーの色が画面に映る", edge, near([0x50, 0x54, 0x5f]));

    // 左クリック = トリガー。
    emu.button(Button::Trigger, true);
    page.until("押した", |s| {
        num(s, &["state", "counts", "pointerdown"]) == 1.0
    });
    emu.button(Button::Trigger, false);
    page.until("クリックした", |s| s["state"]["clicks"] == 1);
    emu.until_color("クリックの結果が画面に映る", edge, near([0x2e, 0xcc, 0x71]));

    // 矢印キー = 右手のスティック。下へ倒すと一覧が下へ動く。
    // 一覧の中心には行があるので、宛先が一覧そのものになるのは待たない。
    let list = emu.on_screen(&status, page.center("scroll-list"));
    emu.point(Some(list));
    emu.stick(Vec2::new(0.0, -1.0));
    page.until("スクロールした", |s| {
        num(s, &["state", "wheel"]) > 0.0 && num(s, &["state", "scrollTop"]) > 0.0
    });
    emu.stick(Vec2::ZERO);

    // カーネルとの対話はどのランタイムでも同じ。
    click(page, emu, "lock");
    page.until("ロックした", |s| {
        s["state"]["status"]["locked"] == true
    });
    click(page, emu, "lock");
    page.until("ロックを外した", |s| {
        s["state"]["status"]["locked"] == false
    });

    // 右クリック = グリップ: パネルを掴んでマウスで右へ運ぶと、元の場所は背景に戻る。
    let grip = emu.on_screen(&status, (20.0, h - 20.0));
    emu.point(Some(grip));
    emu.button(Button::Grab, true);
    page.until("掴んでいる", |s| {
        s["state"]["status"]["grabbed"] == true
    });
    for i in 1..=10 {
        emu.point(Some(grip + Vec2::new(20.0 * i as f32, 0.0)));
    }
    emu.until_color("パネルが動いた", bottom_left, gray);
    emu.button(Button::Grab, false);
    page.until("離した", |s| s["state"]["status"]["grabbed"] == false);

    // マウスが画面の外へ出ると右手が消え、leave が届く。
    emu.point(None);
    page.until("離れた", |s| s["state"]["hovered"] == false);
}
