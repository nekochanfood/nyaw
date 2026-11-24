use super::*;
use crate::test_util::*;
use subsystems::vr::mock::MockOverlay;

const GRAB: [(&str, f32); 1] = [("squeeze/value", 1.0)];
const LIMITS: ResizeLimits = ResizeLimits {
    min: (400, 300),
    max: (1600, 900),
    keep_aspect: false,
};
/// パネル面の縁とクアッドの縁の間隔（m。1m あたり 1000px）。
const MARGIN: f32 = design::PANEL_MARGIN / 1000.0;

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

/// `from` を掴んで `to` まで動かし、追いついてから離す。
fn drag(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay, from: (f32, f32), to: (f32, f32)) {
    controller(vr, aim(from.0, from.1), &GRAB);
    ov.tick(vr);
    controller(vr, aim(to.0, to.1), &GRAB);
    settle(vr, ov);
    controller(vr, aim(to.0, to.1), &[]);
    ov.tick(vr);
}

/// 画面が今の解像度で描いた絵を送ってきたことにする。
fn draw(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay) {
    let (w, h) = ov.panel(0).resolution;
    ov.submit(&mut vr.backend, 0, w, h, &vec![0; (w * h * 4) as usize]);
}

/// 代わりの板（捨てたものを除く）。
fn stand_in(vr: &Subsystems<MockBackend>) -> Option<&MockOverlay> {
    vr.backend
        .overlays
        .iter()
        .find(|o| o.key.starts_with("nyaw.stand-in") && !o.destroyed)
}

fn wait(motion: Motion) {
    std::thread::sleep(std::time::Duration::from_millis(motion.duration_ms() + 50));
}

#[test]
fn grabbing_a_corner_resizes_both_sides_of_the_moved_axis_on_release() {
    let (mut vr, mut ov) = resizable();
    // 右下の角の近く（角から 5cm 内側）を掴んで右へ 10cm。
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim(0.55, -0.2), &GRAB);
    settle(&mut vr, &mut ov);
    // 掴んでいる間は解像度を変えず（画面に描き直させない）、代わりの板だけが幅 20cm 広がる（左右の両側が 10cm ずつ）。
    assert_eq!(ov.panel(0).resolution, (1000, 500));
    let board = stand_in(&vr).unwrap();
    assert!(board.visible);
    assert!(
        (board.width_m - (1.2 - 2.0 * MARGIN)).abs() < 1e-4,
        "{}",
        board.width_m
    );
    assert!(board.pose.position.truncate().abs_diff_eq(Vec2::ZERO, 1e-5));
    // 離すと、その大きさの解像度になる。中心は動かず、1m あたりのピクセル数も変わらない。
    controller(&mut vr, aim(0.55, -0.2), &[]);
    ov.tick(&mut vr);
    assert_eq!(ov.panel(0).resolution, (1200, 500));
    assert!(ov.panel(0).size.abs_diff_eq(Vec2::new(1.2, 0.5), 1e-4));
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    // 下の角を上へ 5cm: 高さだけが 10cm 縮む（上下の両側が縮む）。
    drag(&mut vr, &mut ov, (0.55, -0.2), (0.55, -0.15));
    assert_eq!(ov.panel(0).resolution, (1200, 400));
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
}

#[test]
fn the_grabbed_corner_follows_the_hand_on_every_corner() {
    for (x, y) in [(0.45, 0.2), (-0.45, 0.2), (-0.45, -0.2), (0.45, -0.2)] {
        let (mut vr, mut ov) = resizable();
        // 角を外へ 5cm ずつ引くと、幅も高さも 10cm ずつ広がる。
        drag(
            &mut vr,
            &mut ov,
            (x, y),
            (x + 0.05 * x.signum(), y + 0.05 * y.signum()),
        );
        assert_eq!(ov.panel(0).resolution, (1100, 600), "({x}, {y})");
        assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    }
}

#[test]
fn resizing_stays_within_the_limits() {
    let (mut vr, mut ov) = resizable();
    drag(&mut vr, &mut ov, (0.45, -0.2), (2.0, -2.0));
    assert_eq!(ov.panel(0).resolution, LIMITS.max);
    // 反対側まで手を動かしても最小より小さくはならない（1.6m × 0.9m の右下の角から）。
    drag(&mut vr, &mut ov, (0.75, -0.4), (-0.5, 0.5));
    assert_eq!(ov.panel(0).resolution, LIMITS.min);
    // 範囲の外で動かした分は溜めない（0.4m × 0.3m の右下の角を外へ出してから戻すと、戻した所の大きさになる）。
    controller(&mut vr, aim(0.15, -0.1), &GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim(2.0, -2.0), &GRAB);
    settle(&mut vr, &mut ov);
    drag(&mut vr, &mut ov, (2.0, -2.0), (0.25, -0.15));
    assert_eq!(ov.panel(0).resolution, (600, 400));
}

#[test]
fn resizing_eases_toward_the_hand() {
    let (mut vr, mut ov) = resizable();
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim(0.55, -0.2), &GRAB);
    ov.tick(&mut vr);
    // 1 tick では目標（幅 1.2m）まで跳ばず、途中にいる。
    let width = |vr: &Subsystems<MockBackend>| stand_in(vr).unwrap().width_m + 2.0 * MARGIN;
    let w = width(&vr);
    assert!(w > 1.0 + 1e-3 && w < 1.2 - 1e-3, "{w}");
    settle(&mut vr, &mut ov);
    assert!((width(&vr) - 1.2).abs() < 1e-4);
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
    // 横へだけ動かしても、縦横比（2:1）を保って両方の軸が広がる。
    drag(&mut vr, &mut ov, (0.45, -0.2), (0.55, -0.2));
    let (w, h) = ov.panel(0).resolution;
    assert!(w > 1000 && w == 2 * h, "{w} × {h}");
    assert_at(&ov, &vr, Vec3::new(0.0, 0.0, -1.0));
    // 範囲の端でも比率を保つ（幅が上限 1600px に届いた所で止まり、高さは 800px）。
    let corner = |ov: &Overlay| ov.panel(0).size / 2.0 - Vec2::splat(0.05);
    let c = corner(&ov);
    drag(&mut vr, &mut ov, (c.x, -c.y), (2.0, -2.0));
    assert_eq!(ov.panel(0).resolution, (1600, 800));
    let c = corner(&ov);
    drag(&mut vr, &mut ov, (c.x, -c.y), (-0.5, 0.5));
    assert_eq!(ov.panel(0).resolution, (600, 300));
}

#[test]
fn the_content_fades_to_a_stand_in_until_it_is_redrawn_at_the_new_size() {
    let (mut vr, mut ov) = resizable();
    draw(&mut vr, &mut ov);
    advance(&mut ov, &mut vr, 1.0);
    let content = |vr: &Subsystems<MockBackend>| vr.backend.overlays[0].alpha;
    assert_eq!(content(&vr), 1.0);
    // 角を掴むと、中身が消えて半透明の黒い板に切り替わる。
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    assert!(stand_in(&vr).is_some_and(|o| o.visible));
    wait(Motion::Exit);
    controller(&mut vr, aim(0.55, -0.2), &GRAB);
    settle(&mut vr, &mut ov);
    assert!(content(&vr) < 1e-6, "{}", content(&vr));
    let board = stand_in(&vr).unwrap();
    assert!((board.alpha - 1.0).abs() < 1e-6);
    // 板は中身より手前に描く。絵は半透明の黒で、角は丸い。
    assert!(board.pose.position.z > -1.0);
    let tex = board.texture.as_ref().unwrap();
    assert_eq!(tex.pixel(tex.width / 2, tex.height / 2), [0, 0, 0, 153]);
    assert_eq!(tex.pixel(0, 0)[3], 0);
    // 離して解像度を変えても、新しい大きさの絵が届くまでは板のまま（前の大きさの絵を伸ばして見せない）。
    controller(&mut vr, aim(0.55, -0.2), &[]);
    ov.tick(&mut vr);
    assert_eq!(ov.panel(0).resolution, (1200, 500));
    wait(Motion::Enter);
    ov.tick(&mut vr);
    assert!(content(&vr) < 1e-6);
    // 届いたら中身へ戻り、板は捨てる。
    draw(&mut vr, &mut ov);
    ov.tick(&mut vr);
    wait(Motion::Enter);
    ov.tick(&mut vr);
    assert_eq!(content(&vr), 1.0);
    assert!((vr.backend.overlays[0].width_m - 1.2).abs() < 1e-4);
    assert!(stand_in(&vr).is_none());
}

#[test]
fn the_content_comes_back_even_if_no_new_picture_arrives() {
    let (mut vr, mut ov) = resizable();
    drag(&mut vr, &mut ov, (0.45, -0.2), (0.55, -0.2));
    std::thread::sleep(std::time::Duration::from_secs_f32(STAND_IN_WAIT + 0.05));
    ov.tick(&mut vr);
    wait(Motion::Enter);
    ov.tick(&mut vr);
    assert!(stand_in(&vr).is_none());
    assert!((vr.backend.overlays[0].alpha - 1.0).abs() < 1e-6);
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
    assert_eq!(ov.panel(0).resolution, (1000, 500));
    assert_at(&ov, &vr, Vec3::new(0.3, 0.0, -1.0));
    assert!(stand_in(&vr).is_none());
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
    assert_eq!(ov.panel(0).resolution, (1000, 500));
    assert_at(&ov, &vr, Vec3::new(0.3, 0.0, -1.0));
}

#[test]
fn the_corner_line_fades_in_at_the_pointed_corner() {
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
    // 左上の角を指すと、パネル面の角（影の余白の内側）に、窓 1px = パネル 1px の大きさでフェードインする。
    controller(&mut vr, aim(-0.45, 0.2), &[]);
    ov.tick(&mut vr);
    let o = &vr.backend.overlays[line];
    assert!(o.visible && o.alpha < 1.0, "{}", o.alpha);
    wait(Motion::Enter);
    ov.tick(&mut vr);
    let o = &vr.backend.overlays[line];
    assert!((o.alpha - 1.0).abs() < 1e-6);
    assert!((o.width_m - CORNER_HANDLE_PX as f32 / 1000.0).abs() < 1e-5);
    let face = Vec2::new(0.5, 0.25) - Vec2::splat(MARGIN);
    assert!(
        o.pose
            .position
            .truncate()
            .abs_diff_eq(Vec2::new(-face.x, face.y), 1e-4)
    );
    // ナビゲーションバーと重なっても前に出ないよう、はまる所の見本と同じだけ奥に置き、描画順もパネルの下にする。
    assert!((o.pose.position.z - (-1.0 - GHOST_SINK)).abs() < 1e-5);
    assert!(o.sort < vr.backend.overlays[0].sort);
    // 右下の線を半回転させて左上に置く。
    let roll = o.pose.orientation * Vec3::X;
    assert!(roll.abs_diff_eq(Vec3::NEG_X, 1e-4), "{roll}");
    // 掴んで大きさを変えている間は、代わりの板の角に付いてくる。
    assert!(!ov.corner_held(Hand::Right));
    controller(&mut vr, aim(-0.45, 0.2), &GRAB);
    ov.tick(&mut vr);
    assert!(ov.corner_held(Hand::Right));
    controller(&mut vr, aim(-0.6, 0.2), &GRAB);
    settle(&mut vr, &mut ov);
    let o = &vr.backend.overlays[line];
    assert!(o.visible);
    assert!(
        (o.pose.position.x + 0.65 - MARGIN).abs() < 1e-4,
        "{}",
        o.pose.position
    );
    // 離して中央を指すと、フェードアウトしてから消える。
    controller(&mut vr, aim(0.0, 0.0), &[]);
    ov.tick(&mut vr);
    assert!(!ov.corner_held(Hand::Right));
    assert!(vr.backend.overlays[line].visible);
    wait(Motion::Exit);
    ov.tick(&mut vr);
    assert!(!vr.backend.overlays[line].visible);
}

#[test]
fn the_nav_bar_follows_the_stand_in_while_held() {
    let (mut vr, mut ov) = resizable();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            Pose::IDENTITY,
            Vec2::new(0.4, 0.05),
            (400, 50),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    let top = |ov: &Overlay| ov.panel(bar).pose.position.y + ov.panel(bar).size.y / 2.0;
    ov.tick(&mut vr);
    assert!((top(&ov) + 0.25 - MARGIN).abs() < 1e-4, "{}", top(&ov));
    // 右下の角を掴んで下へ 10cm。解像度はまだ変えないが、バーは代わりの板の下端（10cm 下）に付いてくる。
    controller(&mut vr, aim(0.45, -0.2), &GRAB);
    ov.tick(&mut vr);
    controller(&mut vr, aim(0.45, -0.3), &GRAB);
    settle(&mut vr, &mut ov);
    assert_eq!(ov.panel(0).resolution, (1000, 500));
    assert!((top(&ov) + 0.35 - MARGIN).abs() < 1e-4, "{}", top(&ov));
    // 離して解像度を変えた後も同じ所にいる。
    controller(&mut vr, aim(0.45, -0.3), &[]);
    ov.tick(&mut vr);
    assert_eq!(ov.panel(0).resolution, (1000, 700));
    assert!((top(&ov) + 0.35 - MARGIN).abs() < 1e-4, "{}", top(&ov));
}

#[test]
fn the_stand_in_is_a_rounded_rect_of_translucent_black() {
    let rgba = rounded_rect((20, 10), 4.0, 0.5);
    let alpha = |x: u32, y: u32| rgba[((y * 20 + x) * 4 + 3) as usize];
    assert_eq!(alpha(10, 5), 128);
    assert_eq!(alpha(0, 5), 128);
    assert_eq!(alpha(0, 0), 0);
    assert!(alpha(1, 1) < 128);
    assert!(rgba.chunks(4).all(|p| p[..3] == [0, 0, 0]));
}
