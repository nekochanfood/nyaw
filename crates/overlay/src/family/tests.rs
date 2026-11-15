use super::*;
use crate::test_util::*;

#[test]
fn nav_bar_hangs_under_its_owner_at_a_constant_apparent_size() {
    let (mut vr, mut ov) = setup();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            Pose::IDENTITY,
            Vec2::new(0.2, 0.05),
            (200, 50),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    for z in [-1.0, -3.0] {
        ov.set_pose(0, at(0.0, 0.0, z));
        ov.tick(&mut vr);
        let p = ov.panel(bar);
        // 頭（原点）から見て、持ち主の地の下端（影の余白の内側）の左右中央に、バーの上端の左右中央が付く。
        let anchor = Vec3::new(0.0, -0.5 + PANEL_MARGIN / 100.0, z);
        let top = p.pose.position + Vec3::Y * p.size.y / 2.0 - Vec3::Z * HUD_LIFT;
        assert!(
            top.normalize().abs_diff_eq(anchor.normalize(), 1e-5),
            "{top}"
        );
        // バーの面は持ち主から HUD_LIFT だけ手前（Z ファイティングしない）。
        assert!((p.pose.position.z - (z + HUD_LIFT)).abs() < 1e-5);
        // 頭から見た大きさは距離によらない（1m 先で 0.2m）。
        assert!((p.size.x / top.length() - 0.2).abs() < 1e-5);
        assert!((vr.backend.overlays[bar].width_m - p.size.x).abs() < 1e-5);
    }
    // 持ち主を指せば、バーも指されている扱い（広がる）。どこも指さなければ縮む。
    assert!(!ov.pointed(bar));
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert!(ov.pointed(0) && ov.pointed(bar));
    // バーを掴むと持ち主ごと動き、持ち主を閉じるとバーも閉じる。
    let bar_center = ov.panel(bar).pose.position;
    controller(
        &mut vr,
        Pose::new(
            Vec3::ZERO,
            Quat::from_rotation_arc(Vec3::NEG_Z, bar_center.normalize()),
        ),
        &[("squeeze/value", 1.0)],
    );
    ov.tick(&mut vr);
    assert!(ov.grabbed(0));
    assert!(ov.pointed(bar), "掴んでいる間も広がったまま");
    ov.close(&mut vr.backend, 0);
    assert!(ov.panel(bar).closed);
}

#[test]
fn nav_bar_enters_and_leaves_with_its_owner() {
    let (mut vr, mut ov) = setup();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            Pose::IDENTITY,
            Vec2::new(0.2, 0.05),
            (2, 2),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    let ratio = |ov: &Overlay, id| ov.panel(id).displayed_size().x / ov.panel(id).size.x;
    let rgba = [255u8; 100 * 100 * 4];
    ov.submit(&mut vr.backend, 0, 100, 100, &rgba);
    ov.tick(&mut vr);
    // バーの描画は後から届いても、自分では入場せず持ち主の途中から一緒に出る。
    ov.submit(&mut vr.backend, bar, 2, 2, &[255u8; 16]);
    ov.tick(&mut vr);
    assert!(ratio(&ov, 0) < 1.0);
    assert!((ratio(&ov, bar) - ratio(&ov, 0)).abs() < 1e-6);
    assert_eq!(vr.backend.overlays[bar].alpha, vr.backend.overlays[0].alpha);
    // 閉じるときも一緒に消え、持ち主が消えきるまでは残る。
    ov.close(&mut vr.backend, 0);
    ov.tick(&mut vr);
    assert!(ov.panel(bar).closed);
    assert!((ratio(&ov, bar) - ratio(&ov, 0)).abs() < 1e-6);
    assert_eq!(vr.backend.overlays[bar].alpha, vr.backend.overlays[0].alpha);
}

#[test]
fn idle_nav_bar_gives_its_overlay_back_and_comes_back_when_pointed() {
    let (mut vr, mut ov) = setup();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            Pose::IDENTITY,
            Vec2::new(0.2, 0.05),
            (2, 2),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    ov.set_animated(&mut vr.backend, 0, false);
    ov.set_animated(&mut vr.backend, bar, false);
    ov.submit(&mut vr.backend, 0, 2, 2, &[255u8; 16]);
    ov.submit(&mut vr.backend, bar, 2, 2, &[7u8; 16]);
    let idle = |ov: &mut Overlay| ov.panels[bar].pointed_at -= NAV_BAR_LINGER;
    // 指している間は、いくら経っても消えない。
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    idle(&mut ov);
    ov.tick(&mut vr);
    assert!(ov.pointed(bar) && !ov.panel(bar).surface.parked());
    // 指さないまましばらく経つと消え、ランタイムのオーバーレイを手放す。パネルは残り、見えていない扱い。
    vr.backend.frame.controllers.clear();
    ov.tick(&mut vr);
    assert!(!ov.panel(bar).surface.parked());
    idle(&mut ov);
    ov.tick(&mut vr);
    assert!(ov.panel(bar).surface.parked());
    assert!(vr.backend.overlays[bar].destroyed);
    assert!(!ov.panel(bar).closed && ov.present_ids().contains(&bar));
    assert!(!ov.in_view(bar));
    // 手放している間に届いた描画は送らずに覚えておく。
    ov.submit(&mut vr.backend, bar, 2, 2, &[9u8; 16]);
    ov.tick(&mut vr);
    assert_eq!(vr.backend.overlays.len(), 2);
    // 持ち主を指すと、同じキー・描画順で作り直し、覚えておいた描画で現れる。
    controller(&mut vr, Pose::IDENTITY, &[]);
    ov.tick(&mut vr);
    assert!(!ov.panel(bar).surface.parked());
    assert_eq!(ov.panel(bar).overlay_id(), OverlayId(2));
    let o = &vr.backend.overlays[2];
    assert_eq!((o.key.as_str(), o.sort, o.alpha), ("bar", HUD_SORT, 1.0));
    assert_eq!(o.texture.as_ref().unwrap().rgba, [9u8; 16]);
    assert_eq!(o.pose, ov.panel(bar).pose);
    assert!(!o.destroyed);
    // 動きがあれば、薄れるのに時間をかける（しばらく経った瞬間にはまだ出ている）。
    ov.set_animated(&mut vr.backend, bar, true);
    vr.backend.frame.controllers.clear();
    ov.tick(&mut vr);
    idle(&mut ov);
    ov.tick(&mut vr);
    assert!(!ov.panel(bar).surface.parked());
    assert!(vr.backend.overlays[2].alpha > 0.99);
}

#[test]
fn hidden_docked_panel_keeps_its_nav_bar_size_while_undocking() {
    let (mut vr, mut ov) = with_slot_child();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "nav",
            Pose::IDENTITY,
            Vec2::new(0.2, 0.05),
            (200, 50),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 1);
    ov.tick(&mut vr);
    let size = ov.panel(bar).size;
    ov.set_hidden(&mut vr.backend, 1, true);
    ov.set_parent(&mut vr.backend, 1, None);
    ov.tick(&mut vr);
    assert_eq!(ov.panel(bar).size, size);
}

#[test]
fn child_follows_its_parent() {
    let (mut vr, mut ov) = with_child(false);
    ov.set_pose(0, at(1.0, 2.0, -1.0));
    ov.tick(&mut vr);
    assert!(
        ov.panel(1)
            .pose
            .position
            .abs_diff_eq(Vec3::new(1.0, 2.0, -0.9), 1e-5)
    );
    assert!(
        vr.backend.overlays[1]
            .pose
            .position
            .abs_diff_eq(Vec3::new(1.0, 2.0, -0.9), 1e-5)
    );
    // 取り付けた親にも付いてくる。
    ov.attach(0, Device::Hmd, at(0.0, 0.0, -1.0));
    vr.backend.frame.hmd = at(3.0, 0.0, 0.0);
    ov.tick(&mut vr);
    assert!(
        ov.panel(1)
            .pose
            .position
            .abs_diff_eq(Vec3::new(3.0, 0.0, -0.9), 1e-5)
    );
}

#[test]
fn grabbing_the_child_carries_the_parent_only_when_asked() {
    for carry in [true, false] {
        let (mut vr, mut ov) = with_child(carry);
        // 子（手前）を掴んで右へ運ぶ。
        controller(&mut vr, Pose::IDENTITY, &[("squeeze/value", 1.0)]);
        ov.tick(&mut vr);
        controller(&mut vr, at(1.0, 0.0, 0.0), &[("squeeze/value", 1.0)]);
        settle(&mut vr, &mut ov);
        assert!(ov.panel(1).pose.position.x > 0.9);
        assert_eq!(ov.panel(0).pose.position.x > 0.9, carry, "carry = {carry}");
        // 離したあとも、親が動けば子は新しい相対位置のまま付いてくる。
        controller(&mut vr, at(1.0, 0.0, 0.0), &[]);
        ov.tick(&mut vr);
        let (parent, child) = (ov.panel(0).pose, ov.panel(1).pose.position);
        ov.set_pose(0, Pose::new(parent.position + Vec3::Y, parent.orientation));
        ov.tick(&mut vr);
        assert!(ov.panel(1).pose.position.abs_diff_eq(child + Vec3::Y, 1e-4));
    }
}

#[test]
fn parent_is_dimmed_and_blocked_while_a_child_is_open() {
    let (mut vr, mut ov) = with_child(false);
    // ランタイムが明るさを掛けられないときは、画素を暗くして送る。
    vr.backend.unsupported_tint = true;
    ov.set_animated(&mut vr.backend, 0, false);
    ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
    ov.tick(&mut vr);
    let pixel =
        |vr: &Subsystems<MockBackend>| vr.backend.overlays[0].texture.as_ref().unwrap().pixel(0, 0);
    assert_eq!(pixel(&vr), [110, 55, 28, 255]);
    // 子の外（親だけがある場所）を押しても、親には届かない。
    controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
    assert!(ov.tick(&mut vr).is_empty());
    controller(&mut vr, at(0.45, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    // 子が閉じると戻る。
    ov.close(&mut vr.backend, 1);
    ov.tick(&mut vr);
    assert_eq!(pixel(&vr), [200, 100, 50, 255]);
    controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
    assert!(!ov.tick(&mut vr).is_empty());
}

#[test]
fn slot_child_stands_on_its_parent_without_dimming_or_blocking_it() {
    let (mut vr, mut ov) = with_child(true);
    let tilt = 0.5;
    if let Some(p) = ov.panels[1].parent.as_mut() {
        (p.docked, p.slot) = (true, Some(tilt));
    }
    ov.set_animated(&mut vr.backend, 0, false);
    ov.submit(&mut vr.backend, 0, 1, 1, &[200, 100, 50, 255]);
    ov.tick(&mut vr);
    // 親は暗くならず、押せる。
    assert_eq!(ov.panel(0).dim, 0.0);
    controller(&mut vr, at(0.45, -0.45, 0.0), &[("trigger/value", 1.0)]);
    assert!(!ov.tick(&mut vr).is_empty());
    // 子は親の上端の上に、親から見て `tilt` だけ起こして立つ。地の下端は上端から隙間の分だけ上。
    let (parent, child) = (ov.panel(0).pose, ov.panel(1));
    let local = parent.inverse() * child.pose;
    assert!(
        local
            .orientation
            .abs_diff_eq(Quat::from_rotation_x(tilt), 1e-5)
    );
    let up = local.orientation * Vec3::Y;
    let bottom = local.position - up * (child.size.y / 2.0 - face_margin(child));
    let top = 0.5 - face_margin(ov.panel(0));
    assert!(
        bottom.abs_diff_eq(Vec3::Y * top + up * SLOT_GAP, 1e-4),
        "{bottom}"
    );
    // 親を動かすと付いてくる。
    ov.set_pose(0, at(0.0, 1.0, -1.0));
    ov.tick(&mut vr);
    assert!(
        (ov.panel(0).pose.inverse() * ov.panel(1).pose)
            .position
            .abs_diff_eq(local.position, 1e-4)
    );
}

#[test]
fn undocking_a_slot_child_pulls_it_away_from_its_parent() {
    let (mut vr, mut ov) = with_child(true);
    let slot = Parent {
        panel: 0,
        carry: true,
        docked: true,
        slot: Some(0.0),
        ..Default::default()
    };
    ov.set_parent(&mut vr.backend, 1, Some(slot));
    ov.set_animated(&mut vr.backend, 1, false);
    ov.tick(&mut vr);
    let docked = ov.panel(1).pose;
    ov.set_parent(&mut vr.backend, 1, None);
    ov.tick(&mut vr);
    let pulled = docked * Pose::new(Vec3::Z * UNDOCK_PULL, Quat::IDENTITY);
    assert!(ov.panel(1).pose.position.abs_diff_eq(pulled.position, 1e-4));
    // 外したあとは親に付いていかない。
    ov.set_pose(0, at(0.0, 1.0, -1.0));
    ov.tick(&mut vr);
    assert!(ov.panel(1).pose.position.abs_diff_eq(pulled.position, 1e-4));
}

#[test]
fn grabbing_the_parent_moves_the_slot_child_and_resizes_nothing() {
    const RESIZE: &str = subsystems::vr::RESIZE_INPUT;
    let (mut vr, mut ov) = with_slot_child();
    let sizes = (ov.panel(0).size, ov.panel(1).size);
    // 親だけがある所を掴むと、上の子を掴んだことになる。
    let p = ov.panel(0).pose.position;
    controller(
        &mut vr,
        at(p.x + 0.45, p.y - 0.45, 0.0),
        &[("squeeze/value", 1.0)],
    );
    ov.tick(&mut vr);
    assert!(ov.grabbed(1) && !ov.grabbed(0));
    // 拡大縮小を入れても、掴んだもの（親）とは違うので大きさは変わらない。
    controller(
        &mut vr,
        at(p.x + 1.45, p.y - 0.45, 0.0),
        &[("squeeze/value", 1.0), (RESIZE, 1.0)],
    );
    settle(&mut vr, &mut ov);
    assert_eq!((ov.panel(0).size, ov.panel(1).size), sizes);
    // 子は頭に向けて垂直に立ち、親は子の下に付いてくる。
    assert!(ov.panel(1).pose.position.x > 0.9);
    let local = ov.panel(0).pose.inverse() * ov.panel(1).pose;
    let expected = ov.slot_offset(1, 0, 0.0);
    assert!(
        local.position.abs_diff_eq(expected.position, 1e-3),
        "{local:?}"
    );
}

#[test]
fn a_stream_in_a_slot_keeps_its_height_and_follows_the_aspect_in_width() {
    let (_vr, mut ov) = with_slot_child();
    let height = ov.panel(1).size.y;
    ov.set_content_resolution(1, (1920, 1080));
    assert_eq!(ov.panel(1).size.y, height);
    assert!((ov.panel(1).size.x - height * 16.0 / 9.0).abs() < 1e-5);
    // はめていなければ、これまでどおり幅を保つ。
    let width = ov.panel(0).size.x;
    ov.set_content_resolution(0, (1920, 1080));
    assert_eq!(ov.panel(0).size.x, width);
}

#[test]
fn fitting_height_restores_a_stream_captured_before_docking() {
    let (mut vr, mut ov) = with_slot_child();
    ov.set_parent(&mut vr.backend, 1, None);
    ov.set_content_resolution(1, (1920, 1080));
    assert!(ov.panel(1).size.y < 0.768);
    ov.set_parent(
        &mut vr.backend,
        1,
        Some(Parent {
            panel: 0,
            docked: true,
            slot: Some(0.0),
            ..Default::default()
        }),
    );
    ov.fit_height(&mut vr.backend, 1, 0.768);
    assert!((ov.panel(1).size.y - 0.768).abs() < 1e-5);
    assert!((ov.panel(1).size.x - 0.768 * 16.0 / 9.0).abs() < 1e-5);
}

/// はめた画面が入れ替わりで消える（隠す・閉じる）ときは、後ろへ下がる。外しても手前へ出さない。
#[test]
fn a_slot_child_retreats_while_it_fades_away() {
    let (mut vr, mut ov) = with_slot_child();
    ov.set_animated(&mut vr.backend, 1, true);
    ov.submit(&mut vr.backend, 1, 1, 1, &[200, 100, 50, 255]);
    ov.panels[1].opacity = 1.0;
    let before = ov.panel(1).pose;
    ov.set_hidden(&mut vr.backend, 1, true);
    ov.set_parent(&mut vr.backend, 1, None);
    let glide = &ov.glides[&1];
    let back = before * Pose::new(Vec3::NEG_Z * SLOT_RETREAT, Quat::IDENTITY);
    assert!(
        glide
            .to
            .is_some_and(|to| to.position.abs_diff_eq(back.position, 1e-5))
    );
    assert!(
        ov.panel(1).pose.position.abs_diff_eq(before.position, 1e-5),
        "手前へ出さない"
    );
}

/// ポップオーバー（固定して外側を押すと閉じる子）は、ほかのパネルより前の描画順。
#[test]
fn popovers_are_drawn_in_front_of_panels() {
    let (mut vr, mut ov) = with_child(true);
    ov.set_parent(
        &mut vr.backend,
        1,
        Some(Parent {
            panel: 0,
            carry: true,
            docked: true,
            dismiss: true,
            ..Default::default()
        }),
    );
    assert_eq!(vr.backend.overlays[1].sort, HUD_SORT);
    assert_eq!(vr.backend.overlays[0].sort, PANEL_SORT);
}

#[test]
fn docked_panels_nav_bar_keeps_its_size_at_any_distance() {
    let (mut vr, mut ov) = with_slot_child();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            Pose::IDENTITY,
            Vec2::new(0.4, 0.05),
            (40, 5),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 1);
    ov.tick(&mut vr);
    let near = ov.panel(bar).size;
    vr.backend.frame.hmd.position = Vec3::Z * 3.0;
    ov.tick(&mut vr);
    assert!(ov.panel(bar).size.abs_diff_eq(near, 1e-5));
    assert!(
        near.abs_diff_eq(Vec2::new(0.4, 0.05), 1e-5),
        "1m 先での大きさのまま"
    );
}

#[test]
fn resizing_a_slot_child_keeps_its_bottom_on_the_parent() {
    const RESIZE: &str = subsystems::vr::RESIZE_INPUT;
    let (mut vr, mut ov) = with_slot_child();
    // 実際の画素密度で影の余白を計算し、親と子の面が重ならないようにする。
    ov.panels[0].resolution = (1000, 1000);
    ov.panels[1].resolution = (400, 400);
    ov.tick(&mut vr);
    let eye_level = ov.panel(1).pose.position.y;
    let parent_pose = ov.panel(0).pose;
    ov.set_pose(
        0,
        Pose::new(
            parent_pose.position - Vec3::Y * eye_level,
            parent_pose.orientation,
        ),
    );
    ov.tick(&mut vr);
    // 子を掴んで（掴んだものそのものなので）大きくする。
    let c = ov.panel(1).pose.position;
    let aim = at(c.x, c.y, 0.0);
    controller(&mut vr, aim, &[("squeeze/value", 1.0)]);
    ov.tick(&mut vr);
    assert!(ov.grabbed(1));
    let parent = ov.panel(0).pose.position;
    let size = ov.panel(1).size;
    controller(&mut vr, aim, &[("squeeze/value", 1.0), (RESIZE, 1.0)]);
    for _ in 0..5 {
        ov.tick(&mut vr);
    }
    assert!(ov.panel(1).size.x > size.x);
    // 大きくしても親はほぼ動かない（頭を向くためのわずかな回転はある）。
    assert!(
        ov.panel(0).pose.position.abs_diff_eq(parent, 1e-2),
        "{} -> {}",
        parent,
        ov.panel(0).pose.position
    );
    let held = ov.panel(1).pose.position;
    controller(&mut vr, aim, &[]);
    ov.tick(&mut vr);
    assert!(ov.panel(1).pose.position.abs_diff_eq(held, 1e-3));
}

#[test]
fn hiding_a_parent_hides_its_children_and_stops_input_until_shown() {
    let (mut vr, mut ov) = with_child(true);
    ov.set_animated(&mut vr.backend, 0, false);
    ov.set_animated(&mut vr.backend, 1, false);
    ov.set_hidden(&mut vr.backend, 0, true);
    ov.tick(&mut vr);
    assert!(ov.panel(0).hidden && ov.panel(1).hidden && !ov.panel(0).closed);
    assert_eq!(ov.panel(0).opacity, 0.0);
    controller(&mut vr, at(0.0, 0.0, 0.0), &[("trigger/value", 1.0)]);
    assert!(ov.tick(&mut vr).is_empty());
    controller(&mut vr, at(0.0, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    ov.set_hidden(&mut vr.backend, 0, false);
    ov.tick(&mut vr);
    assert!(!ov.panel(1).hidden && ov.panel(0).opacity == 1.0);
    controller(&mut vr, at(0.0, 0.0, 0.0), &[("trigger/value", 1.0)]);
    assert!(!ov.tick(&mut vr).is_empty());
}

#[test]
fn hiding_a_dialog_restores_its_parents_brightness() {
    let (mut vr, mut ov) = with_child(true);
    ov.tick(&mut vr);
    assert_eq!(ov.panel(0).dim_to, DIM);
    ov.set_hidden(&mut vr.backend, 1, true);
    ov.tick(&mut vr);
    assert_eq!(ov.panel(0).dim_to, 0.0);
}

#[test]
fn hud_child_sits_in_the_top_left_corner_without_taking_input() {
    let (mut vr, mut ov) = with_child(false);
    ov.panels[1].parent.as_mut().unwrap().hud = true;
    ov.tick(&mut vr);
    let corner = Vec3::new(-0.3, 0.3, -1.0 + HUD_LIFT);
    assert!(ov.panel(1).pose.position.abs_diff_eq(corner, 1e-5));
    // 親の縦横比が変わっても角に付いていく。
    ov.panels[0].size = Vec2::new(1.0, 0.5);
    ov.tick(&mut vr);
    let corner = Vec3::new(-0.3, 0.05, -1.0 + HUD_LIFT);
    assert!(ov.panel(1).pose.position.abs_diff_eq(corner, 1e-5));
    // 重なった所を押しても、レーザーは素通りして親へ届く（親は止められない）。
    controller(&mut vr, at(-0.3, 0.05, 0.0), &[("trigger/value", 1.0)]);
    let events = ov.tick(&mut vr);
    assert!(!events.is_empty());
    assert!(events.iter().all(|e| e.panel == 0));
}

#[test]
fn pressing_outside_the_child_closes_it_unless_blocked() {
    let press = |ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, x: f32| {
        controller(vr, at(x, 0.0, 0.0), &[]);
        ov.tick(vr);
        controller(vr, at(x, 0.0, 0.0), &[("trigger/value", 1.0)]);
        ov.tick(vr);
    };
    // 子の中を押しても閉じない。
    let (mut vr, mut ov) = with_child(false);
    ov.panels[1].parent.as_mut().unwrap().dismiss = true;
    press(&mut ov, &mut vr, 0.0);
    assert!(!ov.panel(1).closed);
    // 親の上でも、何もない所でも閉じる。
    for x in [0.45, 3.0] {
        let (mut vr, mut ov) = with_child(false);
        ov.panels[1].parent.as_mut().unwrap().dismiss = true;
        press(&mut ov, &mut vr, x);
        assert!(ov.panel(1).closed && !ov.panel(0).closed, "x = {x}");
    }
    // ブロックした子は閉じない。
    let (mut vr, mut ov) = with_child(false);
    press(&mut ov, &mut vr, 0.45);
    assert!(!ov.panel(1).closed);
    // 閉じた押下は、その下の親には届かない（押したままでも、離しても）。
    let (mut vr, mut ov) = with_child(false);
    ov.panels[1].parent.as_mut().unwrap().dismiss = true;
    let mut seen = Vec::new();
    controller(&mut vr, at(0.45, 0.0, 0.0), &[]);
    seen.extend(ov.tick(&mut vr));
    for _ in 0..3 {
        controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
        seen.extend(ov.tick(&mut vr));
    }
    controller(&mut vr, at(0.45, 0.0, 0.0), &[]);
    seen.extend(ov.tick(&mut vr));
    assert!(ov.panel(1).closed);
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e.event, PointerEvent::Down { .. } | PointerEvent::Up { .. })),
        "{seen:?}"
    );
    // 離したあとは、親は普通に押せる。
    controller(&mut vr, at(0.45, 0.0, 0.0), &[("trigger/value", 1.0)]);
    assert!(
        ov.tick(&mut vr)
            .iter()
            .any(|e| matches!(e.event, PointerEvent::Down { .. }))
    );
}

#[test]
fn pressing_a_nested_child_keeps_its_parent_dialog() {
    let (mut vr, mut ov) = with_child(false);
    ov.panels[1].parent.as_mut().unwrap().dismiss = true;
    let inner = ov
        .add_panel(
            &mut vr.backend,
            "inner",
            at(0.0, 0.0, -0.8),
            Vec2::new(0.2, 0.2),
            (20, 20),
        )
        .unwrap();
    ov.set_parent(
        &mut vr.backend,
        inner,
        Some(Parent {
            panel: 1,
            carry: false,
            dismiss: true,
            hud: false,
            docked: false,
            slot: None,
            stack: false,
        }),
    );
    controller(&mut vr, at(0.0, 0.0, 0.0), &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    assert!(!ov.panel(1).closed && !ov.panel(inner).closed);
    // 外側の子を押すと内側の子だけが閉じる。
    controller(&mut vr, at(0.15, 0.0, 0.0), &[]);
    ov.tick(&mut vr);
    controller(&mut vr, at(0.15, 0.0, 0.0), &[("trigger/value", 1.0)]);
    ov.tick(&mut vr);
    assert!(ov.panel(inner).closed && !ov.panel(1).closed);
}

#[test]
fn closing_the_parent_closes_its_children_and_cycles_are_refused() {
    let (mut vr, mut ov) = with_child(false);
    ov.set_parent(
        &mut vr.backend,
        0,
        Some(Parent {
            panel: 1,
            carry: false,
            dismiss: false,
            hud: false,
            docked: false,
            slot: None,
            stack: false,
        }),
    );
    assert_eq!(
        ov.panel(0).parent,
        None,
        "a child cannot be its parent's parent"
    );
    ov.close(&mut vr.backend, 0);
    assert!(ov.panel(1).closed);
}

/// 通知の組: 親（0 番。バー）・はめた画面（1 番）・別の親（2 番。手首）と、掴めない小さな通知（3 番。描画済み）。
fn with_toast() -> (Subsystems<MockBackend>, Overlay, PanelId) {
    let (mut vr, mut ov) = with_slot_child();
    let wrist = ov
        .add_panel(
            &mut vr.backend,
            "wrist",
            at(0.3, -0.4, -0.4),
            Vec2::splat(0.1),
            (40, 40),
        )
        .unwrap();
    let toast = ov
        .add_panel(
            &mut vr.backend,
            "toast",
            at(0.0, 0.3, -1.5),
            Vec2::new(0.3, 0.1),
            (300, 100),
        )
        .unwrap();
    ov.set_locked(&mut vr.backend, toast, true);
    ov.submit(&mut vr.backend, toast, 1, 1, &[200, 100, 50, 255]);
    ov.panels[toast].opacity = 1.0;
    ov.animations.clear();
    assert_eq!(wrist, 2);
    (vr, ov, toast)
}

/// 動いているものを終わらせ、積む子が追いつくまで tick する（1 tick は少なくとも 1/120 秒進む）。
fn finish_glides(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay) {
    for _ in 0..120 {
        ov.tick(vr);
    }
}

fn stack_on(panel: PanelId) -> Option<Parent> {
    Some(Parent {
        panel,
        carry: true,
        docked: true,
        slot: Some(0.0),
        stack: true,
        ..Default::default()
    })
}

/// 立てた子が別の親の上へ移ると、跳ばずに滑らかに動き、動いている間に変えた大きさも一緒に変わっていく。
#[test]
fn a_slot_child_glides_with_its_size_to_another_parent() {
    let (mut vr, mut ov, toast) = with_toast();
    ov.set_parent(&mut vr.backend, toast, stack_on(2));
    ov.fit_height(&mut vr.backend, toast, 0.05);
    // 次のフレームが遅れても（動きより長く止まっても）、跳ばずに元の所から動き出す。
    std::thread::sleep(std::time::Duration::from_millis(400));
    ov.tick(&mut vr);
    let on_wrist = |ov: &Overlay| {
        ov.panel(toast)
            .pose
            .position
            .distance(ov.panel(2).pose.position)
    };
    assert!(on_wrist(&ov) > 0.3, "最初は元の所から動き出す");
    finish_glides(&mut vr, &mut ov);
    assert!(on_wrist(&ov) < 0.15, "手首の上に収まる");
    let before = ov.panel(toast).pose.position;
    assert!((ov.panel(toast).size.y - 0.05).abs() < 1e-5);
    // 手首からバーの上へ移るときも、跳ばずに動き出す。
    ov.set_parent(&mut vr.backend, toast, stack_on(0));
    ov.fit_height(&mut vr.backend, toast, 0.1);
    assert!(ov.glides.contains_key(&toast));
    ov.tick(&mut vr);
    assert!(ov.panel(toast).pose.position.distance(before) < 0.2);
    assert!(ov.panel(toast).size.y < 0.1, "大きさも少しずつ");
    // 隠すと大きさは変えた後のものになる。
    ov.set_hidden(&mut vr.backend, toast, true);
    assert!((ov.panel(toast).size.y - 0.1).abs() < 1e-5);
}

/// 積む子は、はめた画面があればその上端の上に、無ければ親の上端の上に立つ。
#[test]
fn a_stacked_child_stands_on_the_docked_screen() {
    let (mut vr, mut ov, toast) = with_toast();
    ov.set_animated(&mut vr.backend, toast, false);
    ov.set_parent(&mut vr.backend, toast, stack_on(0));
    ov.tick(&mut vr);
    let (screen, t) = (ov.panel(1), ov.panel(toast));
    let screen_top = screen.pose.position.y + screen.size.y / 2.0 - face_margin(screen);
    let toast_bottom = t.pose.position.y - t.size.y / 2.0 + face_margin(t);
    assert!(
        (toast_bottom - screen_top - SLOT_GAP).abs() < 1e-4,
        "{toast_bottom} {screen_top}"
    );
    // 画面が隠れると、バーの上（画面の立っていた所）へ下りていく。
    ov.set_hidden(&mut vr.backend, 1, true);
    finish_glides(&mut vr, &mut ov);
    let (bar, t) = (ov.panel(0), ov.panel(toast));
    let bar_top = bar.pose.position.y + bar.size.y / 2.0 - face_margin(bar);
    let toast_bottom = t.pose.position.y - t.size.y / 2.0 + face_margin(t);
    assert!(
        (toast_bottom - bar_top - SLOT_GAP).abs() < 1e-3,
        "{toast_bottom} {bar_top}"
    );
}

/// 掴めない子（積んだ通知）があっても、バーを掴むとはめた画面を掴んだことになる。
#[test]
fn a_locked_docked_child_does_not_change_what_a_grab_moves() {
    let (mut vr, mut ov, toast) = with_toast();
    ov.set_parent(&mut vr.backend, toast, stack_on(0));
    ov.tick(&mut vr);
    let frame = vr.poll();
    assert_eq!(
        crate::hand::grab_target(&ov.panels, &ov.present, 0, &frame).0,
        1
    );
    ov.set_parent(&mut vr.backend, 1, None);
    ov.set_parent(&mut vr.backend, toast, stack_on(2));
    assert_eq!(
        crate::hand::grab_target(&ov.panels, &ov.present, 2, &frame),
        (2, Pose::IDENTITY)
    );
}
/// デバイスに付いて動くパネルのナビゲーションバーと子（ポップオーバー）も、同じデバイスに相対で付く（ランタイムが根元と
/// 同じ姿勢で動かすので遅れない）。根元が付いていなければ付けない。
#[test]
fn nav_bar_and_children_follow_the_device_their_root_rides_on() {
    let (mut vr, mut ov) = with_child(true);
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            Pose::IDENTITY,
            Vec2::new(0.2, 0.05),
            (200, 50),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, bar, 0);
    ov.tick(&mut vr);
    assert_eq!(ov.panels[bar].follow, None);
    assert_eq!(ov.panels[1].follow, None);
    let device = Device::Controller(Hand::Right);
    ov.attach(0, device.clone(), at(0.0, 0.2, -0.5));
    let hand = Pose::new(Vec3::new(0.3, -0.2, -0.4), Quat::from_rotation_y(0.5));
    controller(&mut vr, hand, &[]);
    ov.tick(&mut vr);
    for id in [bar, 1] {
        let (d, offset) = ov.panels[id].follow.clone().expect("follows the device");
        assert_eq!(d, device);
        let pose = hand * offset;
        let p = ov.panels[id].pose;
        assert!(pose.position.abs_diff_eq(p.position, 1e-5), "{id}");
        assert!(pose.orientation.abs_diff_eq(p.orientation, 1e-5), "{id}");
    }
}
