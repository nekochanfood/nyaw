//! 実ランタイムでの E2E: ランタイムを起動 → 繋ぐ → オーバーレイへ毎フレーム違う絵を送る →
//! Testing 画面を操作し、その描画を実ランタイムのオーバーレイへ送る →
//! （OpenVR のみ）ランタイムから読み戻して届いたか確かめる → 後片付けでプロセスが消えたか確かめる。
//! 実ランタイムにはコントローラが無い（null ドライバ / simulated）ので、Testing はマウスで操作する。
//!
//! 起動に数秒かかり画面にも出るので、`#[ignore]` と同じく `--ignored` を付けたときだけ流す。
//! 入っていないランタイムはスキップする（`cargo runtimes` で入れる）。
//!
//! ```text
//! cargo test -p testkit --test runtimes -- --ignored           全部
//! cargo test -p testkit --test runtimes -- --ignored monado    名前で絞る
//! ```

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use testing::{BUTTON_ACTIVE_BG, Testing};
use testkit::runtimes::{self, Kind, LaunchError};
use testkit::{Driver, NoVrKit};
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
            println!("ignored（--ignored で流す）");
            skipped += 1;
            continue;
        }
        // SAFETY: harness = false の main はシングルスレッド
        let mut rt = match unsafe { runtimes::launch(kind) } {
            Ok(rt) => rt,
            Err(e @ (LaunchError::NotInstalled(_) | LaunchError::AlreadyRunning(_))) => {
                println!("skipped（{e}）");
                skipped += 1;
                continue;
            }
            Err(e) => {
                println!("FAILED\n  起動できない: {e}");
                failed += 1;
                continue;
            }
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut vr = rt.connect().unwrap_or_else(|e| panic!("繋がらない: {e}"));
            overlay_reaches_runtime(vr.as_mut());
            home_runs_on_runtime(vr);
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
                println!("FAILED\n  後片付けのあとも {kind} のプロセスが残っている");
                failed += 1;
            }
            Err(_) => {
                println!("FAILED（ログ: {log:?}）");
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
        .expect("オーバーレイを作れない");
    vr.set_overlay_pose(id, Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY));

    // 毎フレーム違う絵を、実際のパネルの大きさ（1024x768）で 90Hz で 1 秒送り続けても詰まらないこと。
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
    assert!(frames > 30, "1 秒で {frames} フレームしか送れない");

    // 最後に送った絵がランタイムに届いていること。OpenXR には読み戻す手段が無い。
    if vr.runtime() == Runtime::OpenVr {
        for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            let rgba = color.repeat(64 * 64);
            vr.submit_overlay_pixels(id, 64, 64, &rgba);
            assert!(
                read_back(vr, id, &rgba) == rgba,
                "ランタイムの画像が送ったもの（{color:?}）と違う"
            );
        }
    }
}

/// Testing 画面を実ランタイム上で操作する: ボタンでパネルを HMD に取り付けてロックし、
/// その結果（光ったボタン）がランタイムのオーバーレイに届いていること（OpenVR のみ読み戻せる）。
fn home_runs_on_runtime(vr: Box<dyn VrBackend>) {
    let runtime = vr.runtime();
    let mut cx = gpui::TestAppContext::single();
    let mut kit = NoVrKit::with_backend(&mut cx, vr, |_, _| Testing::default());
    kit.click("attach-hmd").click("lock").hover("click-me");
    let p = kit.kernel.panel(kit.panel);
    assert_eq!(
        (p.attached.as_ref().map(|a| &a.0), p.locked),
        (Some(&Device::Hmd), true),
        "Testing のボタンがパネルに効いていない"
    );
    // 実ランタイムのフレームを回しながら、HMD に付いたパネルへ描画を送り続ける。
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
    panic!("ランタイムのパネルでロックのボタンが光っていない（{got:06x?}）");
}

/// コンポジタの取り込みは非同期なので、届くまで少し待つ。届かなければ最後に読めたものを返す。
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
