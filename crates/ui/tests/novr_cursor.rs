//! novr（エミュレーター）でのポインタの見た目の E2E。本番と同じ `ui::run_with` に GPUI の Testing とビームを載せ、
//! タッチで動かさずに押し続けると、カーソルの周りに長押しの進み具合のリング（ドーナツ型。丸は穴に収まる）が満ちていき、
//! エミュレーターの画面に映ることを確かめる。実際の GPU で描いて読み戻すので、画面のある環境で `--ignored` のときだけ流す。

mod common;

use std::f32::consts::TAU;
use std::process::ExitCode;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Driven, Emu};
use kernel::vr::novr::Button;
use kernel::vr::{Quat, Vec3};
use kernel::{Kernel, Pose, Vec2, VrBackend};
use testing::Testing;
use ui::PanelSpec;

const SCREEN: (u32, u32) = (1280, 720);
const PANEL: Vec3 = Vec3::new(0.0, 0.0, -1.0);

fn main() -> ExitCode {
    if !common::should_run("novr_cursor") {
        return ExitCode::SUCCESS;
    }
    let (backend, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    // 既定（タッチ）のまま。
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    let mut panels = vec![PanelSpec::new(
        Pose::new(PANEL, Quat::IDENTITY),
        |id, _, _| Testing::new(id),
    )];
    panels.extend(beam::specs());
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&emu))).is_ok();
        println!("novr_cursor: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        on_web: None,
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, webpages::FILES, hooks);
    common::exit_code("novr_cursor", &result)
}

fn brightness(c: [u8; 3]) -> u32 {
    c.iter().map(|&v| v as u32).sum::<u32>() / 3
}

fn scenario(emu: &Emu) {
    // 起動とパネルの最初の描画を待つ。
    std::thread::sleep(Duration::from_secs(2));
    // パネルの右下寄り（ボタンの無い背景）を指す。
    let at = emu
        .run(|e| e.project(PANEL + Vec3::new(0.3, -0.2, 0.0)))
        .expect("パネルは目の前");
    emu.point(Some(at));
    std::thread::sleep(Duration::from_millis(300));
    // リングの半径（画面の px）: カーソルのパネルは 1m 先で 36mm、リングの中心線はその 1/3（80/240 px）。
    let ring = emu.run(move |e| {
        let edge = e
            .project(PANEL + Vec3::new(0.3 + 0.036 / 3.0, -0.2, 0.0))
            .unwrap();
        edge.x - at.x
    });
    let on_ring = |turn: f32| {
        let a = (turn - 0.25) * TAU;
        at + Vec2::new(ring * a.cos(), ring * a.sin())
    };
    // 押す前はリングが無い（12 時の位置はパネルの背景のまま）。
    let before = emu.color(on_ring(0.02));

    emu.button(Button::Trigger, true);
    std::thread::sleep(Duration::from_millis(250));
    // 半分ほど満ちた: 満ちた側（12 時の少し先）は明るく、まだの側（10 時ごろ）は下地で暗い。
    let filled = emu.color(on_ring(0.08));
    let empty = emu.color(on_ring(0.85));
    emu.button(Button::Trigger, false);
    println!("リング: 押す前 {before:02x?} / 満ちた側 {filled:02x?} / まだの側 {empty:02x?}");
    assert!(
        brightness(filled) > brightness(before) + 40,
        "満ちた弧が明るく映る（{before:02x?} → {filled:02x?}）"
    );
    assert!(
        brightness(filled) > brightness(empty) + 40,
        "満ちた側とまだの側で違う（{filled:02x?} / {empty:02x?}）"
    );
    // 離すとリングは消える。
    std::thread::sleep(Duration::from_millis(200));
    let after = emu.color(on_ring(0.08));
    assert!(
        brightness(after) + 40 < brightness(filled),
        "離すと消える（{after:02x?}）"
    );
}
