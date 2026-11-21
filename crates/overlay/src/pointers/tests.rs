use super::*;
use crate::test_util::*;

#[test]
fn laser_rot_lays_texture_along_aim_facing_up() {
    assert!((LASER_ROT * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-6));
    assert!((LASER_ROT * Vec3::Z).abs_diff_eq(Vec3::Y, 1e-6));
}

#[test]
fn tooltip_goes_on_the_hand_side_as_seen_from_the_head() {
    let (point, up) = (Vec3::new(0.0, 1.5, -1.0), Vec3::Y);
    let eye = Vec3::new(0.0, 1.6, 0.0);
    // 右手を腰の高さに構え上から見下ろす。レーザーは下方から入射するため Below。
    let low = Vec3::new(0.3, 1.0, -0.3);
    assert_eq!(anchor(Anchor::Above, eye, low, point, up), Anchor::Below);
    // 手を頭より高く掲げて見上げる。Above。
    let high = Vec3::new(0.3, 2.2, -0.3);
    assert_eq!(anchor(Anchor::Below, eye, high, point, up), Anchor::Above);
    // 手がほぼ水平の場合は前回の向きを維持する。
    let side = Vec3::new(0.6, 1.5, -0.6);
    assert_eq!(anchor(Anchor::Below, eye, side, point, up), Anchor::Below);
    assert_eq!(anchor(Anchor::Above, eye, side, point, up), Anchor::Above);
    // 目の位置から指す（novr）場合はレーザーが不可視のため前回を維持する。
    assert_eq!(anchor(Anchor::Below, eye, eye, point, up), Anchor::Below);
}

#[test]
fn tooltip_follows_the_cursor_in_front_of_it() {
    let (mut vr, mut ov, _, cursor) = with_pointers();
    let tip = ov
        .add_panel(
            &mut vr.backend,
            "t",
            Pose::IDENTITY,
            Vec2::new(0.02, 0.02),
            (8, 8),
        )
        .unwrap();
    ov.set_pointer(&mut vr.backend, tip, Hand::Right, Pointer::Tooltip);
    // 手を目より下に配置し正面のパネルを指す。
    controller(
        &mut vr,
        Pose::new(Vec3::new(0.0, -0.4, 0.0), Quat::IDENTITY),
        &[],
    );
    ov.tick(&mut vr);
    let (c, t) = (&vr.backend.overlays[cursor], &vr.backend.overlays[tip]);
    assert!(t.visible && c.visible);
    assert!((t.width_m - c.width_m).abs() < 1e-5, "カーソルと同一縮尺");
    let (pc, pt) = (ov.panel(cursor).pose.position, ov.panel(tip).pose.position);
    assert!((pc.x - pt.x).abs() < 1e-5 && (pc.y - pt.y).abs() < 1e-5);
    assert!(pt.z > pc.z, "カーソルより手前");
    assert_eq!(ov.tooltip_anchor(Hand::Right), Anchor::Below);
}

/// OS カーソルを持つパネルを指すとカーソルに代えて OS カーソル画像をホットスポット基準で最近傍拡大して表示する。
#[test]
fn os_cursor_replaces_the_cursor_at_its_hotspot() {
    let (mut vr, mut ov, _, cursor) = with_pointers();
    let red = [255, 0, 0, 255];
    let mut rgba = vec![0; 16];
    rgba[..4].copy_from_slice(&red);
    let image = subsystems::desktop::Icon {
        width: 2,
        height: 2,
        rgba: rgba.into(),
    };
    let image = Arc::new(Cursor {
        image,
        hotspot: (0, 0),
    });
    // OS カーソルを持たない手（左）が指す場合、nyaw カーソルのまま。
    ov.set_os_cursor(0, Some((Hand::Left, image.clone())));
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert!(vr.backend.overlays[cursor].visible);
    ov.set_os_cursor(0, Some((Hand::Right, image)));
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[cursor].visible);
    let os = vr.backend.overlays.last().unwrap();
    // パネルは 1m/100px。面はホットスポット中心の 4px 四方で、中心がヒット点。
    let px = 0.01;
    assert!(os.visible && (os.width_m - 4.0 * px).abs() < 1e-6);
    assert!(
        os.pose
            .position
            .abs_diff_eq(Vec3::new(0.0, 0.0, -1.0 + CURSOR_LIFT), 1e-5)
    );
    // 1px を 128 倍に拡大。ホットスポット（左上画素の角）がテクスチャ中心に配置される。
    let t = os.texture.as_ref().unwrap();
    assert_eq!((t.width, t.height), (512, 512));
    let at = |x: usize, y: usize| &t.rgba[(y * 512 + x) * 4..][..4];
    assert_eq!((at(256, 256), at(383, 383)), (&red[..], &red[..]));
    assert_eq!((at(255, 256), at(384, 256)), (&[0; 4][..], &[0; 4][..]));
    // 形状が変わっても（ホットスポット位置が異なっても）位置とサイズは不変。
    let before = (os.pose, os.width_m);
    let image = subsystems::desktop::Icon {
        width: 2,
        height: 2,
        rgba: vec![255; 16].into(),
    };
    ov.set_os_cursor(
        0,
        Some((
            Hand::Right,
            Arc::new(Cursor {
                image,
                hotspot: (1, 1),
            }),
        )),
    );
    ov.tick(&mut vr);
    let os = vr.backend.overlays.last().unwrap();
    assert_eq!((os.pose, os.width_m), before);
    ov.set_os_cursor(0, None);
    ov.tick(&mut vr);
    assert!(vr.backend.overlays[cursor].visible);
    assert!(!vr.backend.overlays.last().unwrap().visible);
}

/// 非表示ポインタは最終表示位置に残存しても不可視扱い（再描画しない）。
#[test]
fn hidden_pointer_is_not_in_view() {
    let (mut vr, mut ov, _, cursor) = with_pointers();
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert!(ov.in_view(cursor));
    vr.backend.frame.controllers.clear();
    ov.tick(&mut vr);
    assert!(!ov.in_view(cursor));
}

#[test]
fn laser_stops_short_of_hit_and_cursor_floats_in_front_of_panel() {
    let (mut vr, mut ov, laser, cursor) = with_pointers();
    controller(&mut vr, Pose::IDENTITY, &[]);
    // ポインタパネルはレーザー手前にあってもヒット対象外。
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move {
            pos: Vec2::new(50.0, 50.0)
        }]
    );
    let l = &vr.backend.overlays[laser];
    let shown = (1.0 - LASER_GAP).min(LASER_LENGTH);
    assert!(l.visible && (l.width_m - shown).abs() < 1e-5);
    assert!(
        l.pose
            .position
            .abs_diff_eq(Vec3::new(0.0, 0.0, -shown / 2.0), 1e-5)
    );
    let c = &vr.backend.overlays[cursor];
    assert!(c.visible);
    assert!(
        c.pose
            .position
            .abs_diff_eq(Vec3::new(0.0, 0.0, -1.0 + CURSOR_LIFT), 1e-5)
    );
    // カーソルはパネル（0）およびレーザーより手前に描画する。
    assert!(c.sort > l.sort && l.sort > vr.backend.overlays[0].sort);
}

#[test]
fn laser_turns_its_face_to_head_around_its_axis() {
    let (mut vr, mut ov, laser, _) = with_pointers();
    // 頭がレーザーの右側方にある配置。
    vr.backend.frame.hmd = at(1.0, 0.0, -0.5);
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    let q = vr.backend.overlays[laser].pose.orientation;
    assert!((q * Vec3::Z).abs_diff_eq(Vec3::X, 1e-5), "{}", q * Vec3::Z);
    assert!((q * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-5));
}

#[test]
fn laser_keeps_its_thickness_and_cursor_its_apparent_size() {
    let (mut vr, mut ov, laser, cursor) = with_pointers();
    assert_eq!(vr.backend.overlays[laser].height_m, Some(0.02));
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    // 頭（原点）から 1m 先のカーソルは基準サイズ、2m 先なら 2 倍。
    let near = vr.backend.overlays[cursor].width_m;
    assert!((near - 0.02).abs() < 1e-5, "{near}");
    vr.backend.frame.hmd = at(0.0, 0.0, 1.0);
    ov.tick(&mut vr);
    assert!((vr.backend.overlays[cursor].width_m - near * 2.0).abs() < 1e-4);
    // カーソルサイズ倍率が適用される（NaN 等の非数値は等倍扱い）。
    ov.set_cursor_scale(1.5);
    ov.tick(&mut vr);
    assert!((vr.backend.overlays[cursor].width_m - near * 3.0).abs() < 1e-4);
    ov.set_cursor_scale(f32::NAN);
    ov.tick(&mut vr);
    assert!((vr.backend.overlays[cursor].width_m - near * 2.0).abs() < 1e-4);
}

#[test]
fn drag_visual_shows_only_while_dragging_at_given_width() {
    let (mut vr, mut ov, _, cursor) = with_pointers();
    let drag = ov
        .add_panel(
            &mut vr.backend,
            "d",
            Pose::IDENTITY,
            Vec2::splat(0.4),
            (400, 400),
        )
        .unwrap();
    ov.set_pointer(&mut vr.backend, drag, Hand::Right, Pointer::Drag);
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[drag].visible);
    ov.set_drag(Hand::Right, Some(0.25));
    ov.tick(&mut vr);
    let d = &vr.backend.overlays[drag];
    assert!(d.visible && (d.width_m - 0.25).abs() < 1e-6);
    assert!(
        d.pose
            .position
            .abs_diff_eq(Vec3::new(0.0, 0.0, -1.0 + DRAG_LIFT), 1e-5)
    );
    // 描画順: パネル < ドラッグ表示 < カーソル。
    assert!(vr.backend.overlays[0].sort < d.sort && d.sort < vr.backend.overlays[cursor].sort);
    ov.set_drag(Hand::Right, None);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[drag].visible);
}

#[test]
fn missing_panel_hides_laser_and_cursor() {
    let (mut vr, mut ov, laser, cursor) = with_pointers();
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert!(vr.backend.overlays[laser].visible && vr.backend.overlays[cursor].visible);
    controller(&mut vr, at(5.0, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[laser].visible && !vr.backend.overlays[cursor].visible);
    // グラブ中はパネル外を向いていてもレーザーを表示する。
    controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    controller(&mut vr, at(0.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
    vr.backend.frame.controllers[0].aim.orientation = Quat::from_rotation_y(1.0);
    ov.tick(&mut vr);
    assert!(vr.backend.overlays[laser].visible);
}
