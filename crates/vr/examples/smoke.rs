//! OpenVR 実ランタイムへの手動の疎通確認: 接続 → オーバーレイ作成 → 単色の画像を送信 →
//! ランタイムから読み戻して色が届いたか確かめる → 数秒間 poll してコントローラを表示する。
//! `cargo run -p vr --example smoke`

use std::ffi::{CString, c_void};
use std::time::{Duration, Instant};

use openvr_bindings as sys;
use vr::{OverlayId, Pose, Quat, Runtime, Vec3};

const W: u32 = 64;
const H: u32 = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut vr = vr::connect(Some(Runtime::OpenVr))?;
    println!("connected: {:?}", vr.runtime());

    let id = vr.create_overlay("nyaw.smoke", 0.5)?;
    vr.set_overlay_pose(id, Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY));

    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let rgba = color.repeat((W * H) as usize);
        // 同じ画像を何度送ってもランタイムには 1 回しか届かない（詰まらない）こと。
        for _ in 0..300 {
            vr.submit_overlay_pixels(id, W, H, &rgba);
        }
        // コンポジタの取り込みは非同期なので、届くまで少し待つ。
        let start = Instant::now();
        let got = loop {
            vr.poll();
            match read_back(id) {
                Ok(got) => break got,
                Err(e) if start.elapsed() > Duration::from_secs(5) => return Err(e.into()),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
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

/// バックエンドの外から OpenVR に直接問い合わせ、オーバーレイが今持っている画像を得る。
fn read_back(id: OverlayId) -> Result<Vec<u8>, String> {
    let name = CString::new(format!(
        "FnTable:{}",
        std::str::from_utf8(sys::IVROverlay_Version)
            .unwrap()
            .trim_end_matches('\0')
    ))
    .unwrap();
    let mut err = 0;
    let table = unsafe { sys::VR_GetGenericInterface(name.as_ptr(), &mut err) };
    let overlay = unsafe { &*(table as *const sys::VR_IVROverlay_FnTable) };
    let mut buf = vec![0u8; (W * H * 4) as usize];
    let (mut w, mut h) = (0, 0);
    let e = unsafe {
        overlay.GetOverlayImageData.unwrap()(
            id.0,
            buf.as_mut_ptr() as *mut c_void,
            buf.len() as u32,
            &mut w,
            &mut h,
        )
    };
    if e != sys::EVROverlayError_VROverlayError_None {
        return Err(format!("GetOverlayImageData: EVROverlayError {e}"));
    }
    if (w, h) != (W, H) {
        return Err(format!("大きさが違う: {w}x{h}"));
    }
    Ok(buf)
}
