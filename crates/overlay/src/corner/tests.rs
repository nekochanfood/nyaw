use super::*;
use crate::test_util::*;

const GRAB: [(&str, f32); 1] = [("squeeze/value", 1.0)];
const LIMITS: ResizeLimits = ResizeLimits {
    min: (400, 300),
    max: (1600, 900),
    keep_aspect: false,
};

/// 1m × 0.5m（1000 × 500px、1m あたり 1000px）のパネルを正面 1m に置き、角で大きさを変えられるようにする。
fn resizable() -> (Subsystems<MockBackend>, Overlay) {
    let mut vr = Subsystems::with_backend(MockBackend::default());
    let mut ov = Overlay::new();
    ov.add_panel(
        &mut vr.backend,
        "main",
        at(0.0, 0.0, -1.0),
        Vec2::new(1.0, 0.5),
        (1000, 500),
    )
    .unwrap();
    ov.set_pointer_smoothing(0.0);
    ov.set_corner_resize(0, Some(LIMITS));
    (vr, ov)
}

/// 頭の位置から、パネル（z = −1）の面上の点（パネルローカル x, y。m）を指す。
fn aim(x: f32, y: f32) -> Pose {
    Pose::new(
        Vec3::ZERO,
        Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(x, y, -1.0).normalize()),
    )
}

fn size(ov: &Overlay) -> ((u32, u32), Vec2) {
    (ov.panel(0).resolution, ov.panel(0).size)
}

#[test]
fn grabbing_a_corner_resizes_both_sides_of_the_moved_axis() {
    let (mut vr, mut ov) = resizable();
    // 右下の角の近く（角から 5cm 内側）を掴む。
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    // 右へ 10cm: 幅だけが 20cm 広がる（左右の両側が 10cm ずつ）。中心は動かない。
    controller(&mut vr, aim(0.55, -0.2), &GRAB);
    settle(&mut vr, &mut ov);
    let (res, m) = size(&ov);
    assert_eq!(res, (1200, 500));
    assert!(m.abs_diff_eq(Vec2::new(1.2, 0.5), 1e-4), "{m}");
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    // 上へ 5cm: 高さだけが 10cm 縮む（下の角を上へ動かすので上下の両側が縮む）。
    controller(&mut vr, aim(0.55, -0.15), &GRAB);
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, (1200, 400));
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    // 離すとその大きさのまま。
    controller(&mut vr, aim(0.55, -0.15), &[]);
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, (1200, 400));
    // 1m あたりのピクセル数は変わらず、ランタイム上の幅も合わせる。
    let (res, m) = size(&ov);
    assert!((res.0 as f32 / m.x - 1000.0).abs() < 1e-2);
    assert!((res.1 as f32 / m.y - 1000.0).abs() < 1e-2);
    assert!((vr.backend.overlays[0].width_m - 1.2).abs() < 1e-4);
}

#[test]
fn the_grabbed_corner_follows_the_hand_on_every_corner() {
    for (x, y) in [(0.45, 0.2), (-0.45, 0.2), (-0.45, -0.2), (0.45, -0.2)] {
        let (mut vr, mut ov) = resizable();
        controller(&mut vr, aim(x, y), &GRAB);
        ov.tick(&mut vr);
        // 角を外へ 5cm ずつ引くと、幅も高さも 10cm ずつ広がる。
        controller(
            &mut vr,
            aim(x + 0.05 * x.signum(), y + 0.05 * y.signum()),
            &GRAB,
        );
        settle(&mut vr, &mut ov);
        assert_eq!(size(&ov).0, (1100, 600), "({x}, {y})");
        assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    }
}

#[test]
fn resizing_stays_within_the_limits() {
    let (mut vr, mut ov) = resizable();
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim(2.0, -2.0), &GRAB);
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, LIMITS.max);
    // 反対側まで手を動かしても最小より小さくはならない。
    controller(&mut vr, aim(-0.5, 0.5), &GRAB);
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, LIMITS.min);
    // 範囲の外で動かした分は溜めない（戻し始めたらすぐ大きくなる）。
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, (1000, 500));
}

#[test]
fn resizing_eases_toward_the_hand() {
    let (mut vr, mut ov) = resizable();
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim(0.55, -0.2), &GRAB);
    ov.tick(&mut vr);
    // 1 tick では目標（1200px）まで跳ばず、途中にいる。
    let w = size(&ov).0.0;
    assert!(w > 1000 && w < 1200, "{w}");
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0.0, 1200);
}

#[test]
fn grabbing_away_from_the_corners_still_moves_the_panel() {
    let (mut vr, mut ov) = resizable();
    controller(&mut vr, aim(0.0, 0.0), &GRAB);
    ov.tick(&mut vr);
    controller(
        &mut vr,
        Pose::new(Vec3::X * 0.3, aim(0.0, 0.0).orientation),
        &GRAB,
    );
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, (1000, 500));
    assert_at(&ov, &vr, Vec3::new(0.3, 0.0, -1.0));
}

#[test]
fn panels_without_the_option_move_when_grabbed_at_a_corner() {
    let (mut vr, mut ov) = resizable();
    ov.set_corner_resize(0, None);
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    controller(
        &mut vr,
        Pose::new(Vec3::X * 0.3, aim(0.45, -0.2).orientation),
        &GRAB,
    );
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, (1000, 500));
    assert_at(&ov, &vr, Vec3::new(0.3, 0.0, -1.0));
}

#[test]
fn the_corner_line_shows_at_the_pointed_corner() {
    let (mut vr, mut ov) = resizable();
    let line = ov
        .add_panel(
            &mut vr.backend,
            "corner",
            Pose::IDENTITY,
            Vec2::splat(0.1),
            (CORNER_HANDLE_PX, CORNER_HANDLE_PX),
        )
        .unwrap();
    ov.set_pointer(&mut vr.backend, line, Hand::Right, Pointer::Corner);
    // 中央を指している間は出さない。
    controller(&mut vr, aim(0.0, 0.0), &[]);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[line].visible);
    // 左上の角を指すと、パネル面の角（影の余白の内側）に、窓 1px = パネル 1px の大きさで出す。
    controller(&mut vr, aim(-0.45, 0.2), &[]);
    ov.tick(&mut vr);
    let o = &vr.backend.overlays[line];
    assert!(o.visible);
    assert!((o.width_m - CORNER_HANDLE_PX as f32 / 1000.0).abs() < 1e-5);
    let face = Vec2::new(0.5, 0.25) - Vec2::splat(design::PANEL_MARGIN / 1000.0);
    assert!(
        o.pose
            .position
            .truncate()
            .abs_diff_eq(Vec2::new(-face.x, face.y), 1e-4)
    );
    // 右下の線を半回転させて左上に置く。
    let roll = o.pose.orientation * Vec3::X;
    assert!(roll.abs_diff_eq(Vec3::NEG_X, 1e-4), "{roll}");
    // 掴んで大きさを変えている間は、手が角から外れても角に付いてくる。
    controller(&mut vr, aim(-0.45, 0.2), &GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim(-0.6, 0.2), &GRAB);
    settle(&mut vr, &mut ov);
    let o = &vr.backend.overlays[line];
    assert!(o.visible);
    let face = ov.panel(0).size / 2.0 - Vec2::splat(design::PANEL_MARGIN / 1000.0);
    assert!(
        (o.pose.position.x + face.x).abs() < 1e-4,
        "{}",
        o.pose.position
    );
    // 離して中央を指すと消える。
    controller(&mut vr, aim(0.0, 0.0), &[]);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[line].visible);
}

#[test]
fn keeping_the_aspect_resizes_both_axes_together() {
    let (mut vr, mut ov) = resizable();
    ov.set_corner_resize(
        0,
        Some(ResizeLimits {
            keep_aspect: true,
            ..LIMITS
        }),
    );
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    // 横へだけ動かしても、縦横比（2:1）を保って両方の軸が広がる。
    controller(&mut vr, aim(0.55, -0.2), &GRAB);
    settle(&mut vr, &mut ov);
    let (w, h) = size(&ov).0;
    assert!(w > 1000 && w == 2 * h, "{w} × {h}");
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    // 範囲の端でも比率を保つ（幅が上限 1600px に届いた所で止まり、高さは 800px）。
    controller(&mut vr, aim(2.0, -2.0), &GRAB);
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, (1600, 800));
    controller(&mut vr, aim(-0.5, 0.5), &GRAB);
    settle(&mut vr, &mut ov);
    assert_eq!(size(&ov).0, (600, 300));
}
