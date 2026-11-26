use super::*;
use crate::test_util::*;

#[test]
fn scale_keeps_center_and_hit_coordinates_when_resized() {
    let (mut vr, mut ov) = setup();
    let pose = ov.panel(0).pose;
    ov.submit(&mut vr.backend, 0, 1, 1, &[255; 4]);
    ov.set_resolution(&mut vr.backend, 0, (200, 100));
    assert_eq!(vr.backend.overlays[0].width_m, 1.5);
    let p = ov.panel(0);
    assert_eq!(p.pixel_to_world(Vec2::new(100.0, 50.0)), pose.position);
    assert_eq!(
        p.pixel_to_world(Vec2::ZERO),
        pose.transform_point(Vec3::new(-0.75, 0.375, 0.0))
    );
    let (px, _) = hit_test(p, &at(0.375, 0.0, 0.0)).unwrap();
    assert_eq!(px, Vec2::new(150.0, 50.0));
    assert!(hit_test(p, &at(0.8, 0.0, 0.0)).is_none());
    advance(&mut ov, &mut vr, 0.2);
    assert_eq!(vr.backend.overlays[0].width_m, 2.0);
    assert_eq!(ov.panel(0).pose, pose);
}

/// UI スケールは開くパネルの実寸に適用され、解像度（レイアウト）と中心は不変。既存パネルにも新規パネルにも有効。
#[test]
fn scale_multiplies_sizes_and_keeps_centers_and_resolution() {
    let (mut vr, mut ov) = setup();
    assert_eq!(ov.scale(), 1.0);
    let pose = ov.panel(0).pose;
    ov.set_scale(&mut vr.backend, 1.5);
    assert_eq!(ov.panel(0).size, Vec2::splat(1.5));
    assert_eq!(
        (ov.panel(0).pose, ov.panel(0).resolution),
        (pose, (100, 100))
    );
    assert_eq!(vr.backend.overlays[0].width_m, 1.5);
    let later = ov
        .add_panel(
            &mut vr.backend,
            "later",
            at(3.0, 0.0, -1.0),
            Vec2::new(0.4, 0.2),
            (40, 20),
        )
        .unwrap();
    assert!(ov.panel(later).size.abs_diff_eq(Vec2::new(0.6, 0.3), 1e-6));
    assert!((vr.backend.overlays[later].width_m - 0.6).abs() < 1e-6);
    // レーザーは実寸の面にヒットする: 等倍では外れる 0.6m 右も、1.5 倍の面（半幅 0.75m）上にある。
    controller(&mut vr, at(0.6, 0.0, 0.0), &[]);
    let events = kinds(&ov.tick(&mut vr));
    assert_eq!(
        events,
        [PointerEvent::Move {
            pos: Vec2::new(90.0, 50.0)
        }]
    );
    // fit_height に渡す長さも等倍設計の値。
    ov.fit_height(&mut vr.backend, later, 0.4);
    assert!(ov.panel(later).size.abs_diff_eq(Vec2::new(1.2, 0.6), 1e-6));
    // 複数回変更しても累積せず、戻せば元のサイズ。非数は等倍扱い。
    ov.set_scale(&mut vr.backend, 0.7);
    ov.set_scale(&mut vr.backend, f32::NAN);
    assert_eq!(ov.scale(), 1.0);
    assert!(ov.panel(0).size.abs_diff_eq(Vec2::ONE, 1e-6));
    assert!((vr.backend.overlays[0].width_m - 1.0).abs() < 1e-6);
}

/// グループ（親子・ナビバー・グラブ基準）は UI スケール変更後も相似を維持（子は同率で離れる）。
#[test]
fn scale_keeps_a_group_similar() {
    let (mut vr, mut ov) = with_child(true);
    ov.set_grab_anchor(0, Some(Pose::new(Vec3::X * 0.3, Quat::IDENTITY)));
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "nav",
            Pose::IDENTITY,
            Vec2::new(0.2, 0.05),
            (200, 50),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    ov.tick(&mut vr);
    ov.set_scale(&mut vr.backend, 1.5);
    ov.tick(&mut vr);
    let gap = ov.panel(1).pose.position - ov.panel(0).pose.position;
    assert!(gap.abs_diff_eq(Vec3::Z * 0.15, 1e-5), "{gap}");
    assert!(ov.panel(1).size.abs_diff_eq(Vec2::splat(0.6), 1e-6));
    let anchor = ov.panel(0).grab_anchor.unwrap();
    assert!((anchor.position.x - 0.45).abs() < 1e-6, "{anchor:?}");
    assert!(
        ov.panels[bar]
            .nav_bar_size
            .abs_diff_eq(Vec2::new(0.3, 0.075), 1e-6)
    );
    // バーは現在の持ち主の下端に配置されている。
    let top = ov.panel(bar).pose.position.y + ov.panel(bar).size.y / 2.0;
    let bottom = ov.panel(0).pose.position.y - (0.75 - face_margin(ov.panel(0)));
    assert!((top - bottom).abs() < 1e-5, "{top} {bottom}");
    // 後から設定する基準にも適用される。
    ov.set_grab_anchor(0, Some(Pose::new(Vec3::X * 0.1, Quat::IDENTITY)));
    let anchor = ov.panel(0).grab_anchor.unwrap();
    assert!((anchor.position.x - 0.15).abs() < 1e-6, "{anchor:?}");
}

/// グラブ基準を付け替えて移動すると、新しい基準が現在の基準位置に来て向きは不変。位置には UI スケールが適用される。
#[test]
fn moving_the_grab_anchor_keeps_its_place() {
    let (mut vr, mut ov) = setup();
    ov.set_scale(&mut vr.backend, 1.5);
    ov.set_grab_anchor(0, Some(Pose::new(Vec3::X * 0.3, Quat::IDENTITY)));
    let (fixed, facing) = (
        ov.panel(0).pose * ov.panel(0).grab_anchor.unwrap(),
        ov.panel(0).pose.orientation,
    );
    let to = Pose::new(Vec3::new(-0.2, 0.1, 0.0), Quat::IDENTITY);
    ov.move_grab_anchor(0, to);
    let p = ov.panel(0);
    let anchor = p.grab_anchor.unwrap();
    assert!(
        anchor.position.abs_diff_eq(to.position * 1.5, 1e-6),
        "{anchor:?}"
    );
    assert!((p.pose * anchor).position.abs_diff_eq(fixed.position, 1e-5));
    assert_eq!(p.pose.orientation, facing);
}

/// グラブ基準を持つパネル（ダッシュボードのバー）は基準位置を固定してサイズ変更され、再配置しても基準位置は同一。
#[test]
fn scale_keeps_the_grab_anchor_in_place() {
    let (mut vr, mut ov) = setup();
    let anchor = Pose::new(Vec3::new(0.0, 0.4, 0.1), Quat::from_rotation_x(0.3));
    ov.set_grab_anchor(0, Some(anchor));
    let fixed = ov.panel(0).pose * anchor;
    let world = |ov: &Overlay| ov.panel(0).pose * ov.panel(0).grab_anchor.unwrap();
    for scale in [1.5, 0.7, 1.0] {
        ov.set_scale(&mut vr.backend, scale);
        let now = world(&ov);
        assert!(now.position.abs_diff_eq(fixed.position, 1e-5), "{scale}");
        assert!(now.orientation.abs_diff_eq(fixed.orientation, 1e-5));
        // パネル中心は基準から同率で離れる。
        let from_anchor = ov.panel(0).pose.position - fixed.position;
        let authored = at(0.0, 0.0, -1.0).position - fixed.position;
        assert!(from_anchor.abs_diff_eq(authored * scale, 1e-5), "{scale}");
    }
    // 等倍設計の姿勢に再配置: 等倍なら同じ姿勢、倍率があれば基準が等倍時と同じ位置に来る。
    let to = Pose::new(Vec3::new(1.0, 2.0, -3.0), Quat::from_rotation_y(0.5));
    ov.place(0, to);
    assert!(ov.panel(0).pose.position.abs_diff_eq(to.position, 1e-6));
    ov.set_scale(&mut vr.backend, 1.5);
    ov.place(0, to);
    assert!(
        world(&ov)
            .position
            .abs_diff_eq((to * anchor).position, 1e-5)
    );
    assert!(
        ov.panel(0)
            .pose
            .orientation
            .abs_diff_eq(to.orientation, 1e-5)
    );
    // 基準なしのパネルは中心がその姿勢。
    ov.set_grab_anchor(0, None);
    ov.place(0, to);
    assert_eq!(ov.panel(0).pose, to);
}

/// スロット子との隙間も UI スケールに合わせて拡大される（グループが相似のまま拡大）。
#[test]
fn scale_widens_the_slot_gap_with_the_panels() {
    let (mut vr, mut ov) = with_slot_child();
    let gap = |ov: &Overlay| {
        let (outer, inner) = (ov.panel(0), ov.panel(1));
        let top = outer.pose.position.y + outer.size.y / 2.0 - face_margin(outer);
        let bottom = inner.pose.position.y - inner.size.y / 2.0 + face_margin(inner);
        bottom - top
    };
    assert!((gap(&ov) - SLOT_GAP).abs() < 1e-5, "{}", gap(&ov));
    ov.set_scale(&mut vr.backend, 1.5);
    ov.tick(&mut vr);
    assert!((gap(&ov) - SLOT_GAP * 1.5).abs() < 1e-5, "{}", gap(&ov));
}

#[test]
fn resize_keeps_pixel_density_and_hit_mapping() {
    let (mut vr, mut ov) = setup();
    // 100px = 1m のパネルを 200x50 にすると 2m x 0.5m になる。
    ov.set_resolution(&mut vr.backend, 0, (200, 50));
    assert_eq!(ov.panel(0).size, Vec2::new(2.0, 0.5));
    assert_eq!(vr.backend.overlays[0].width_m, 2.0);
    // 中心から右 0.75m・上 0.125m は新解像度で (175, 12.5)。
    controller(
        &mut vr,
        Pose::new(Vec3::new(0.75, 0.125, 0.0), Quat::IDENTITY),
        &[],
    );
    assert_eq!(
        kinds(&ov.tick(&mut vr)),
        [PointerEvent::Move {
            pos: Vec2::new(175.0, 12.5)
        }]
    );
}
