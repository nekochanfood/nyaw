//! 実ランタイムへの手動の疎通確認: 接続 → オーバーレイ作成 → 毎フレーム違う画像を送り続ける →
//! （OpenVR のみ）ランタイムから読み戻して色が届いたか確かめる → 数秒間 poll してコントローラを表示する。
//! `cargo run -p vr --example smoke`（OpenXR は `-- --openxr`）

use std::time::{Duration, Instant};

use vr::{OverlayId, Pose, Quat, Runtime, Vec3};

const W: u32 = 64;
const H: u32 = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = match std::env::args().nth(1).as_deref() {
        Some("--openxr") => Runtime::OpenXr,
        _ => Runtime::OpenVr,
    };
    let mut vr = vr::connect(Some(runtime))?;
    println!("connected: {:?}", vr.runtime());

    let id = vr.create_overlay("nyaw.smoke", 0.5)?;
    vr.set_overlay_pose(id, Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY));

    // アニメーション: 毎フレーム違う絵を 90Hz で 3 秒送り続けても詰まらないこと。
    // 大きさは実際のパネル（1024x768）に合わせる。
    let (aw, ah) = (1024, 768);
    let start = Instant::now();
    let (mut frames, mut busy) = (0u32, Duration::ZERO);
    while start.elapsed() < Duration::from_secs(3) {
        let v = (frames % 256) as u8;
        let rgba = [v, 255 - v, 0, 255].repeat((aw * ah) as usize);
        let t = Instant::now();
        vr.submit_overlay_pixels(id, aw, ah, &rgba);
        busy += t.elapsed();
        vr.poll();
        frames += 1;
        std::thread::sleep(Duration::from_millis(11));
    }
    println!(
        "animated {frames} frames ({aw}x{ah}), submit {:.2} ms/frame",
        busy.as_secs_f64() * 1000.0 / frames as f64
    );

    // 最後に送った絵がランタイムに届いていること。OpenXR には読み戻す手段が無い。
    let colors: &[[u8; 4]] = if runtime == Runtime::OpenVr {
        &[[255, 0, 0, 255], [0, 0, 255, 255]]
    } else {
        &[]
    };
    for color in colors {
        let rgba = color.repeat((W * H) as usize);
        vr.submit_overlay_pixels(id, W, H, &rgba);
        // コンポジタの取り込みは非同期なので、届くまで少し待つ。
        let start = Instant::now();
        let got = loop {
            vr.poll();
            match read_back(id) {
                Ok(got) if got == rgba => break got,
                Err(e) if start.elapsed() > Duration::from_secs(5) => return Err(e.into()),
                Ok(got) if start.elapsed() > Duration::from_secs(5) => break got,
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        println!("sent {color:?}, runtime has {:?}", &got[..4]);
        assert_eq!(got, rgba, "ランタイムが持っている画像が送ったものと違う");
    }

    let start = Instant::now();
    let mut last = None;
    while start.elapsed() < Duration::from_secs(3) {
        let frame = vr.poll();
        let profiles: Vec<_> = frame
            .controllers
            .iter()
            .map(|c| c.profile.clone())
            .collect();
        if last.as_ref() != Some(&profiles) {
            println!("hmd = {:?}, controllers = {profiles:?}", frame.hmd.position);
            last = Some(profiles);
        }
        std::thread::sleep(Duration::from_millis(11));
    }
    println!("ok");
    Ok(())
}

fn read_back(id: OverlayId) -> Result<Vec<u8>, String> {
    let (w, h, rgba) = vr::openvr_overlay_image(id).map_err(|e| e.to_string())?;
    if (w, h) != (W, H) {
        return Err(format!("大きさが違う: {w}x{h}"));
    }
    Ok(rgba)
}
