//! テストで使う共通の道具。テストはそれぞれのモジュールの子（`hand/tests.rs` など）に置く。

use super::*;
pub(crate) use vr::{Quat, RawController, mock::MockBackend};

pub(crate) fn advance(ov: &mut Overlay, vr: &mut Subsystems<MockBackend>, seconds: f32) {
    let now = Instant::now();
    for (_, last) in ov.animations.values_mut() {
        *last = now - std::time::Duration::from_secs_f32(seconds);
    }
    ov.animate(&mut vr.backend, now);
}

pub(crate) fn setup() -> (Subsystems<MockBackend>, Overlay) {
    let mut vr = Subsystems::with_backend(MockBackend::default());
    let mut ov = Overlay::new();
    let pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
    ov.add_panel(
        &mut vr.backend,
        "main",
        pose,
        Vec2::new(1.0, 1.0),
        (100, 100),
    )
    .unwrap();
    (vr, ov)
}

pub(crate) fn controller(vr: &mut Subsystems<MockBackend>, aim: Pose, inputs: &[(&str, f32)]) {
    vr.backend.frame.controllers = vec![RawController {
        hand: Hand::Right,
        profile: "valve/index_controller".into(),
        aim,
        inputs: inputs.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
    }];
}

/// 掴み移動がなめらかに追いつくまで、同じ入力のまま tick する。
pub(crate) fn settle(vr: &mut Subsystems<MockBackend>, ov: &mut Overlay) {
    for _ in 0..60 {
        ov.tick(vr);
    }
}

pub(crate) fn kinds(ev: &[UiEvent]) -> Vec<PointerEvent> {
    ev.iter().map(|e| e.event).collect()
}

/// 親（0 番）の手前に小さな子を置く。
pub(crate) fn with_child(carry: bool) -> (Subsystems<MockBackend>, Overlay) {
    let (mut vr, mut ov) = setup();
    let child = ov
        .add_panel(
            &mut vr.backend,
            "child",
            at(0.0, 0.0, -0.9),
            Vec2::new(0.4, 0.4),
            (40, 40),
        )
        .unwrap();
    ov.set_parent(
        &mut vr.backend,
        child,
        Some(Parent {
            panel: 0,
            carry,
            dismiss: false,
            hud: false,
            docked: false,
            slot: None,
        }),
    );
    (vr, ov)
}

/// 親（ダッシュボードのバー）の上端に立てた子（はめた画面）を持つ組。親は大きさを変えられない。
pub(crate) fn with_slot_child() -> (Subsystems<MockBackend>, Overlay) {
    let (mut vr, mut ov) = with_child(true);
    ov.set_animated(&mut vr.backend, 1, false);
    ov.set_parent(
        &mut vr.backend,
        1,
        Some(Parent {
            panel: 0,
            carry: true,
            docked: true,
            slot: Some(0.0),
            ..Default::default()
        }),
    );
    ov.set_resizable(0, false);
    ov.tick(&mut vr);
    // 子が目の高さの正面に来るよう、親を下げる（掴んだ子は目の方を向くので、目の高さなら回らない）。
    let lift = ov.panel(1).pose.position.y;
    ov.set_pose(0, at(0.0, -lift, -1.0));
    ov.tick(&mut vr);
    (vr, ov)
}

pub(crate) fn at(x: f32, y: f32, z: f32) -> Pose {
    Pose::new(Vec3::new(x, y, z), Quat::IDENTITY)
}

pub(crate) fn assert_at(ov: &Overlay, vr: &Subsystems<MockBackend>, expected: Vec3) {
    let got = ov.panel(0).pose.position;
    assert!(got.abs_diff_eq(expected, 1e-5), "{got}");
    assert!(
        vr.backend.overlays[0]
            .pose
            .position
            .abs_diff_eq(expected, 1e-5)
    );
}

/// パネル（z = -1）の前にレーザーとカーソルを置き、右手で中心を指す。
pub(crate) fn with_pointers() -> (Subsystems<MockBackend>, Overlay, PanelId, PanelId) {
    let (mut vr, mut ov) = setup();
    let mut add = |ov: &mut Overlay, part| {
        let id = ov
            .add_panel(
                &mut vr.backend,
                "p",
                Pose::IDENTITY,
                Vec2::new(0.02, 0.02),
                (8, 8),
            )
            .unwrap();
        ov.set_pointer(&mut vr.backend, id, Hand::Right, part);
        id
    };
    let laser = add(&mut ov, Pointer::Laser);
    let cursor = add(&mut ov, Pointer::Cursor);
    (vr, ov, laser, cursor)
}

pub(crate) fn move_x(ev: &[UiEvent]) -> f32 {
    let PointerEvent::Move { pos } = ev[0].event else {
        panic!()
    };
    pos.x
}
