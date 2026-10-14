//! 起動済み SteamVR にテスト専用パネルを作り、実画像のイン・アウトを読み戻す。
//! `cargo test -p overlay --test animation_runtime -- --ignored`

use std::time::{Duration, Instant};

use overlay::Overlay;
use vr::{Pose, Runtime, Vec2, connect_exact, openvr_overlay_image, openvr_overlay_state};

#[test]
#[ignore = "requires a running SteamVR and a Vulkan GPU"]
fn animation_reaches_steamvr() {
    let mut vr = connect_exact(Runtime::OpenVr).expect("failed to connect to SteamVR");
    let mut ov = Overlay::new();
    let key = format!("nyaw.test.animation.{}", std::process::id());
    let panel = ov
        .add_panel(vr.as_mut(), &key, Pose::IDENTITY, Vec2::ONE, (64, 64))
        .unwrap();
    let id = ov.panel(panel).overlay_id();
    ov.submit(vr.as_mut(), panel, 64, 64, &[255; 64 * 64 * 4]);
    let (_, _, initial) = openvr_overlay_image(id).unwrap();
    assert!(
        initial.iter().all(|v| *v == 255),
        "original image is left unmodified"
    );
    assert_eq!(openvr_overlay_state(id).unwrap(), (0.75, 0.0, true));

    let start = Instant::now();
    let mut intermediate = false;
    while start.elapsed() < Duration::from_millis(300) {
        ov.tick(vr.as_mut());
        let (width, alpha, _) = openvr_overlay_state(id).unwrap();
        intermediate |= 0.0 < alpha && alpha < 1.0;
        assert!((width - (0.75 + 0.25 * alpha)).abs() < 1e-5);
    }
    assert!(intermediate, "no intermediate frame during fade-in");
    let (_, _, full) = openvr_overlay_image(id).unwrap();
    assert!(full.iter().all(|v| *v == 255));
    assert_eq!(ov.panel(panel).displayed_size(), Vec2::ONE);
    assert_eq!(openvr_overlay_state(id).unwrap(), (1.0, 1.0, true));

    ov.close(vr.as_mut(), panel);
    let start = Instant::now();
    let mut intermediate = false;
    while start.elapsed() < Duration::from_millis(250) {
        ov.tick(vr.as_mut());
        let (width, alpha, _) = openvr_overlay_state(id).unwrap();
        intermediate |= 0.0 < alpha && alpha < 1.0;
        assert!((width - (0.75 + 0.25 * alpha)).abs() < 1e-5);
    }
    assert!(intermediate, "no intermediate frame during fade-out");
    let (w, h, hidden) = openvr_overlay_image(id).unwrap();
    assert_eq!((w, h), (1, 1));
    assert_eq!(hidden, [0; 4]);
    assert_eq!(ov.panel(panel).displayed_size(), Vec2::splat(0.75));
    assert_eq!(openvr_overlay_state(id).unwrap(), (0.75, 0.0, false));
}
