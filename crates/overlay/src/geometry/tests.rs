use super::*;

/// 掴み移動の追いつきは経過時間で決まる: 2 tick 分の時間を 1 回で進めても同じ所に来る。
#[test]
fn approach_depends_on_elapsed_time_not_ticks() {
    let (from, to) = (Pose::IDENTITY, Pose::new(Vec3::X, Quat::IDENTITY));
    let twice = approach(approach(from, to, TICK_MIN), to, TICK_MIN);
    let once = approach(from, to, TICK_MIN * 2.0);
    assert!(once.position.abs_diff_eq(twice.position, 1e-6));
}

#[test]
fn facing_points_surface_at_eye() {
    let q = facing(Vec3::new(0.0, 0.0, -1.0), Vec3::ZERO, Vec3::Y).unwrap();
    assert!(q.abs_diff_eq(Quat::IDENTITY, 1e-6));
    assert!(facing(Vec3::ZERO, Vec3::Y, Vec3::Y).is_none());
}
