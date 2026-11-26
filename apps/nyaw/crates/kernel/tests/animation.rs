//! Advances a normal-size panel in real time and checks what the kernel delivers to the backend.

use std::time::Duration;

use kernel::{Kernel, PanelCommand, Pose, Vec2, vr::mock::MockBackend};

#[test]
fn full_size_panel_animates_in_and_out_while_neighbor_opts_out() {
    let mut k = Kernel::with_backend(MockBackend::default(), Default::default());
    let panel = k
        .add_panel("animated", Pose::IDENTITY, Vec2::new(0.8, 0.6), (800, 600))
        .unwrap();
    let neighbor = k
        .add_panel("static", Pose::IDENTITY, Vec2::ONE, (4, 4))
        .unwrap();
    k.apply(neighbor, PanelCommand::Animated(false));
    k.submit(neighbor, 4, 4, &[255; 64]);
    k.submit(panel, 800, 600, &vec![128; 800 * 600 * 4]);
    let mut width = k.subsystems.backend.overlays[panel].width_m;
    assert!((width - 0.6).abs() < 1e-6);
    let mut alpha = 0;
    let mut intermediate = false;
    for _ in 0..25 {
        k.tick();
        let o = &k.subsystems.backend.overlays[panel];
        let next = (o.texture.as_ref().unwrap().pixel(400, 300)[3] as f32 * o.alpha).round() as u8;
        assert!(o.width_m >= width && next >= alpha);
        intermediate |= 0 < next && next < 128;
        width = o.width_m;
        alpha = next;
        std::thread::sleep(Duration::from_millis(11));
    }
    assert!(intermediate);
    assert_eq!((width, alpha), (0.8, 128));
    assert_eq!(
        k.subsystems.backend.overlays[panel].pixel_submissions, 1,
        "fade-in does not re-upload the image"
    );
    assert_eq!(k.panel(panel).pose, Pose::IDENTITY);
    k.apply(panel, PanelCommand::Close);
    assert!(k.status(panel).closed && k.subsystems.backend.overlays[panel].visible);
    let mut intermediate = false;
    for _ in 0..20 {
        k.tick();
        let o = &k.subsystems.backend.overlays[panel];
        let next = (o.texture.as_ref().unwrap().pixel(0, 0)[3] as f32 * o.alpha).round() as u8;
        assert!(o.width_m <= width && next <= alpha);
        intermediate |= 0 < next && next < 128;
        width = o.width_m;
        alpha = next;
        std::thread::sleep(Duration::from_millis(11));
    }
    assert!(intermediate);
    assert!((width - 0.6).abs() < 1e-6);
    assert_eq!(alpha, 0);
    assert!(!k.subsystems.backend.overlays[panel].visible);
    assert_eq!(
        k.subsystems.backend.overlays[panel].pixel_submissions, 2,
        "fade-out uploads only the final transparent 1px"
    );
    assert_eq!(k.panel(panel).pose, Pose::IDENTITY);
    assert!(k.subsystems.backend.overlays[neighbor].visible);
    assert_eq!(k.subsystems.backend.overlays[neighbor].width_m, 1.0);
    assert_eq!(
        k.subsystems.backend.overlays[neighbor]
            .texture
            .as_ref()
            .unwrap()
            .pixel(0, 0),
        [255; 4]
    );
}
