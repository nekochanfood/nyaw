//! E2E: ダッシュボードにドッキングした画面を掴んで引き抜き、近づけて再ドッキングする（本番の `ui::run_with`・novr エミュレーター・実 GPU）。
//! 引いている間・抜けた後・近づけた位置・スナップした位置・再ドッキング後のエミュレーター画面を `target/tmp/dock-*.bmp` に書き出し、
//! ドッキング状態の記録（`ui::docked`）が手の動きに追従することを検証する。`--ignored` のときのみ実行する。

use crate::common;

use std::process::ExitCode;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Driven, Emu};
use dashboard::{Dashboard, Page};
use kernel::vr::Vec3;
use kernel::vr::novr::Button;
use kernel::{Kernel, Language, Setting, VrBackend};

const SCREEN: (u32, u32) = (1280, 720);
/// ドッキングした画面の中心（既定のダッシュボードの距離・高さ）。
const DOCKED: Vec3 = Vec3::new(0.0, -0.1, -kernel::DASHBOARD_DISTANCE);

pub fn main() -> ExitCode {
    if !common::should_run("dock_magnet") {
        return ExitCode::SUCCESS;
    }
    let (backend, emu) = Driven::headless(SCREEN.0, SCREEN.1);
    let kernel = Kernel::with_backend(Box::new(backend) as Box<dyn VrBackend>, Default::default());
    let summon: dashboard::Summon = |from, opened| {
        ui::PanelSpec::spawn(from, |id, _, cx| {
            opened(id, cx);
            testing::Testing::new(id)
        })
    };
    let mut panels = vec![dashboard::spec(|_, _| {}, summon, summon, false)];
    panels.extend(beam::specs());
    let (remote, rx) = channel::<ui::Remote>();
    let result: Arc<Mutex<Option<bool>>> = Arc::default();
    let done = result.clone();
    std::thread::spawn(move || {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&emu, &remote)))
            .is_ok();
        println!("dock_magnet: {}", if ok { "ok" } else { "FAILED" });
        *done.lock().unwrap() = Some(ok);
        common::quit(&remote, ok);
    });
    let hooks = ui::Hooks {
        remote: Some(rx),
        hide_profiler: false,
    };
    ui::run_with(kernel, panels, None, hooks);
    common::exit_code("dock_magnet", &result)
}

/// フレームループで `f` を実行して結果を返す。
fn on_app<R: Send + 'static>(
    remote: &Sender<ui::Remote>,
    f: impl FnOnce(&mut gpui::App) -> R + Send + 'static,
) -> R {
    let (tx, rx) = channel();
    remote
        .send(Box::new(move |cx| {
            let _ = tx.send(f(cx));
        }))
        .expect("frame loop");
    rx.recv_timeout(common::TIMEOUT)
        .expect("frame loop answers")
}

/// エミュレーターの画面を BMP で書き出す。
fn shot(emu: &Emu, name: &str) {
    let (w, h, rgba) = emu.run(|e| e.screenshot());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp")
        .join(format!("dock-{name}.bmp"));
    let mut bmp = vec![0u8; 54];
    bmp[..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&(54 + rgba.len() as u32).to_le_bytes());
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&w.to_le_bytes());
    bmp[22..26].copy_from_slice(&(-(h as i32)).to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
    for p in rgba.as_chunks::<4>().0 {
        bmp.extend_from_slice(&[p[2], p[1], p[0], 255]);
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bmp).unwrap();
}

/// 掴んだまま、ドッキング画面の中心から `to` だけ離れた位置を照準するまで、`from` から段階的に手を移動する。
fn pull(emu: &Emu, from: Vec3, to: Vec3) {
    for i in 1..=15 {
        let at = DOCKED + from.lerp(to, i as f32 / 15.0);
        let p = emu.run(move |e| e.project(at)).expect("in view");
        emu.point(Some(p));
        std::thread::sleep(Duration::from_millis(15));
    }
    std::thread::sleep(Duration::from_millis(500));
}

fn scenario(emu: &Emu, remote: &Sender<ui::Remote>) {
    std::thread::sleep(Duration::from_secs(2));
    on_app(remote, |cx| {
        ui::set(Setting::Language(Language::Ja), cx);
        let bar = cx
            .windows()
            .into_iter()
            .filter_map(|w| w.downcast::<ui::PanelRoot>())
            .find_map(|w| {
                w.read(cx)
                    .ok()
                    .and_then(|r| r.view().downcast::<Dashboard>().ok())
            })
            .expect("the dashboard");
        bar.update(cx, |d, cx| d.toggle(Page::QuickSettings, cx));
    });
    std::thread::sleep(Duration::from_millis(1500));
    let docked = on_app(remote, |cx| ui::docked(cx));
    assert!(docked.is_some(), "the quick settings are docked");
    let at = emu.run(|e| e.project(DOCKED)).expect("in view");
    emu.point(Some(at));
    std::thread::sleep(Duration::from_millis(200));
    emu.button(Button::Grab, true);
    std::thread::sleep(Duration::from_millis(300));
    let x = |m: f32| Vec3::X * m;
    // 少し引いても、ドッキング位置から動かない（まだドッキング中）。
    pull(emu, Vec3::ZERO, x(0.12));
    shot(emu, "pull");
    assert_eq!(on_app(remote, |cx| ui::docked(cx)), docked, "still docked");
    // 引き抜くと独立する。上下に動かしてもプレビューは移動しない。
    pull(emu, x(0.12), x(0.5));
    shot(emu, "torn");
    assert_eq!(on_app(remote, |cx| ui::docked(cx)), None, "pulled off");
    let up = x(0.5) + Vec3::Y * 0.25;
    pull(emu, x(0.5), up);
    shot(emu, "torn-up");
    // 近づけると引き寄せられ、スナップする。
    pull(emu, up, x(0.22));
    shot(emu, "near");
    pull(emu, x(0.22), x(0.03));
    shot(emu, "caught");
    // 解放するとドッキングされる。
    emu.button(Button::Grab, false);
    std::thread::sleep(Duration::from_millis(800));
    shot(emu, "back");
    assert_eq!(on_app(remote, |cx| ui::docked(cx)), docked, "docked again");
}
