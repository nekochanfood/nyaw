//! 実ランタイムでの E2E: ランタイムを起動 → 繋ぐ → オーバーレイへ毎フレーム違う絵を送る →
//! （OpenVR のみ）ランタイムから読み戻して届いたか確かめる → 後片付けでプロセスが消えたか確かめる。
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

use testkit::runtimes::{self, Kind, LaunchError};
use vr::{OverlayId, Pose, Quat, Runtime, Vec3, VrBackend};

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
