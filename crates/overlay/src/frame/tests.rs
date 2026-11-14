use super::*;
use crate::test_util::*;

#[test]
fn entrance_and_exit_reach_backend_without_new_ui_frames() {
    let (mut vr, mut ov) = setup();
    vr.backend.unsupported_alpha = true;
    let pose = ov.panel(0).pose;
    ov.submit(
        &mut vr.backend,
        0,
        2,
        1,
        &[80, 40, 20, 128, 200, 100, 50, 255],
    );
    assert_eq!(vr.backend.overlays[0].width_m, 0.75);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().rgba,
        [0; 8]
    );
    advance(&mut ov, &mut vr, 0.1);
    assert!((vr.backend.overlays[0].width_m - 0.96875).abs() < 1e-6);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
        [70, 35, 18, 112]
    );
    // UI の次の描画が届いても、インを最初からやり直さない。
    ov.submit(
        &mut vr.backend,
        0,
        2,
        1,
        &[80, 40, 20, 128, 200, 100, 50, 255],
    );
    advance(&mut ov, &mut vr, 0.1);
    assert_eq!(vr.backend.overlays[0].width_m, 1.0);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
        [80, 40, 20, 128]
    );
    assert!(ov.animations.is_empty());

    ov.close(&mut vr.backend, 0);
    assert!(ov.panel(0).closed && vr.backend.overlays[0].visible);
    advance(&mut ov, &mut vr, 0.1);
    assert!((vr.backend.overlays[0].width_m - 0.96875).abs() < 1e-6);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
        [70, 35, 18, 112]
    );
    // 閉じた UI から遅れて届く描画は、アウトの絵を上書きしない。
    ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
    advance(&mut ov, &mut vr, 0.1);
    assert!(!vr.backend.overlays[0].visible);
    assert_eq!(vr.backend.overlays[0].width_m, 0.75);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
        [0; 4]
    );
    assert!(!ov.panel(0).surface.has_texture() && ov.animations.is_empty());
    assert_eq!(ov.panel(0).pose, pose);
}

#[test]
fn native_alpha_does_not_modify_or_resubmit_pixels() {
    let (mut vr, mut ov) = setup();
    ov.submit(&mut vr.backend, 0, 1, 1, &[80, 40, 20, 128]);
    assert_eq!(vr.backend.overlays[0].alpha, 0.0);
    advance(&mut ov, &mut vr, 0.1);
    assert_eq!(vr.backend.overlays[0].alpha, 0.875);
    assert_eq!(vr.backend.overlays[0].pixel_submissions, 1);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
        [80, 40, 20, 128]
    );
    ov.submit(&mut vr.backend, 0, 1, 1, &[40, 20, 10, 64]);
    assert_eq!(vr.backend.overlays[0].alpha, 0.875);
    advance(&mut ov, &mut vr, 0.1);
    assert_eq!(vr.backend.overlays[0].alpha, 1.0);
    ov.close(&mut vr.backend, 0);
    advance(&mut ov, &mut vr, 0.1);
    assert_eq!(vr.backend.overlays[0].alpha, 0.875);
    assert_eq!(vr.backend.overlays[0].pixel_submissions, 2);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
        [40, 20, 10, 64]
    );
    advance(&mut ov, &mut vr, 0.1);
    assert_eq!(vr.backend.overlays[0].alpha, 0.0);
    assert!(!vr.backend.overlays[0].visible);
}

#[test]
fn close_interrupts_entrance_and_repeated_close_does_not_restart() {
    let (mut vr, mut ov) = setup();
    ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
    advance(&mut ov, &mut vr, 0.05);
    let before = vr.backend.overlays[0].width_m;
    ov.close(&mut vr.backend, 0);
    assert_eq!(vr.backend.overlays[0].width_m, before);
    controller(&mut vr, Pose::IDENTITY, &[("trigger/value", 1.0)]);
    assert!(ov.tick(&mut vr).is_empty());
    advance(&mut ov, &mut vr, 0.1);
    assert!(vr.backend.overlays[0].width_m < before);
    ov.close(&mut vr.backend, 0);
    advance(&mut ov, &mut vr, 0.1);
    assert!(!vr.backend.overlays[0].visible && ov.animations.is_empty());
}

#[test]
fn closed_panel_history_does_not_stay_in_tick_set() {
    let (mut vr, mut ov) = setup();
    for i in 0..32 {
        let id = ov
            .add_panel(
                &mut vr.backend,
                &format!("closed-{i}"),
                Pose::IDENTITY,
                Vec2::ONE,
                (1, 1),
            )
            .unwrap();
        ov.close(&mut vr.backend, id);
    }
    ov.tick(&mut vr);
    assert_eq!(ov.len(), 33);
    assert_eq!(ov.present_ids(), &[0]);
}

#[test]
fn opt_out_is_per_panel_and_finishes_running_animation() {
    let (mut vr, mut ov) = setup();
    let other = ov
        .add_panel(
            &mut vr.backend,
            "other",
            Pose::IDENTITY,
            Vec2::ONE,
            (100, 100),
        )
        .unwrap();
    ov.set_animated(&mut vr.backend, other, false);
    for id in [0, other] {
        ov.submit(&mut vr.backend, id, 1, 1, &[255; 4]);
    }
    assert_eq!(vr.backend.overlays[0].width_m, 0.75);
    assert_eq!(vr.backend.overlays[other].width_m, 1.0);
    assert_eq!(
        vr.backend.overlays[other]
            .texture
            .as_ref()
            .unwrap()
            .pixel(0, 0),
        [255; 4]
    );
    ov.set_animated(&mut vr.backend, 0, false);
    assert_eq!(vr.backend.overlays[0].width_m, 1.0);
    assert!(ov.animations.is_empty());
    ov.close(&mut vr.backend, other);
    assert!(!vr.backend.overlays[other].visible);
    ov.set_animated(&mut vr.backend, 0, true);
    ov.close(&mut vr.backend, 0);
    assert!(vr.backend.overlays[0].visible);
    ov.set_animated(&mut vr.backend, 0, false);
    assert!(!vr.backend.overlays[0].visible && ov.animations.is_empty());
}

#[test]
fn tick_uses_elapsed_time_and_pointers_opt_out() {
    let (mut vr, mut ov, laser, cursor) = with_pointers();
    for id in [laser, cursor] {
        assert!(!ov.panel(id).animated);
    }
    ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
    ov.animations.get_mut(&0).unwrap().1 = Instant::now() - std::time::Duration::from_millis(250);
    ov.tick(&mut vr);
    assert_eq!(vr.backend.overlays[0].width_m, 1.0);
    assert_eq!(
        vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0),
        [255; 4]
    );
    assert!(ov.animations.is_empty());
}

#[test]
fn submit_reaches_backend() {
    let (mut vr, mut ov) = setup();
    ov.set_animated(&mut vr.backend, 0, false);
    let mut rgba = vec![0; 200 * 200 * 4];
    rgba[..4].copy_from_slice(&[1, 2, 3, 4]);
    ov.submit(&mut vr.backend, 0, 200, 200, &rgba);
    let tex = vr.backend.overlays[0].texture.as_ref().unwrap();
    assert_eq!((tex.width, tex.height), (200, 200));
    assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
}

#[test]
fn dimming_fades_in_and_out_like_the_panels() {
    let (mut vr, mut ov) = with_child(false);
    ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
    ov.animations.clear();
    ov.tick(&mut vr);
    let dim = |ov: &Overlay| ov.panel(0).dim;
    assert_eq!(dim(&ov), 0.0);
    let step = |ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, secs: f32| {
        let now = Instant::now();
        for (_, last) in ov.dims.values_mut() {
            *last = now - std::time::Duration::from_secs_f32(secs);
        }
        ov.animate(&mut vr.backend, now);
    };
    step(&mut ov, &mut vr, 0.1);
    assert!(0.0 < dim(&ov) && dim(&ov) < DIM, "{}", dim(&ov));
    step(&mut ov, &mut vr, 0.1);
    assert_eq!(dim(&ov), DIM);
    assert!(ov.dims.is_empty());
    ov.close(&mut vr.backend, 1);
    ov.tick(&mut vr);
    step(&mut ov, &mut vr, 0.075);
    assert!(0.0 < dim(&ov) && dim(&ov) < DIM);
    step(&mut ov, &mut vr, 0.075);
    assert_eq!(dim(&ov), 0.0);
}

#[test]
fn veil_darkens_the_space_behind_the_panels_while_its_panel_is_open() {
    let (mut vr, mut ov) = setup();
    ov.set_veil(0, true);
    ov.tick(&mut vr);
    assert_eq!(
        vr.backend.overlays.len(),
        1,
        "最初の描画が届くまでは暗くしない"
    );
    ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
    ov.tick(&mut vr);
    fn veil(vr: &Subsystems<MockBackend>) -> &vr::mock::MockOverlay {
        &vr.backend.overlays[1]
    }
    assert!(veil(&vr).sort < vr.backend.overlays[0].sort);
    assert_eq!(veil(&vr).texture.as_ref().unwrap().rgba, [0, 0, 0, 255]);
    let step = |ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, secs: f32| {
        if let Some((_, last)) = &mut ov.veil.fade {
            *last -= std::time::Duration::from_secs_f32(secs);
        }
        ov.tick(vr);
    };
    step(&mut ov, &mut vr, 0.1);
    assert!(0.0 < veil(&vr).alpha && veil(&vr).alpha < VEIL);
    step(&mut ov, &mut vr, 0.2);
    assert_eq!(veil(&vr).alpha, VEIL);
    assert!(veil(&vr).visible);
    ov.close(&mut vr.backend, 0);
    ov.tick(&mut vr);
    step(&mut ov, &mut vr, 0.075);
    assert!(0.0 < veil(&vr).alpha && veil(&vr).alpha < VEIL);
    step(&mut ov, &mut vr, 0.2);
    assert_eq!(veil(&vr).alpha, 0.0);
    assert!(!veil(&vr).visible);
}
