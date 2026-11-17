use super::*;
use crate::test_util::*;

/// 腕時計を見る構え（手首を顔の前に上げ、面をこちらへ向ける）だけを、見ていることにする。
#[test]
fn a_panel_is_gazed_only_while_it_faces_the_head_in_front() {
    let (mut vr, mut ov) = setup();
    let watch = ov
        .add_panel(
            &mut vr.backend,
            "watch",
            Pose::IDENTITY,
            Vec2::splat(0.1),
            (40, 40),
        )
        .unwrap();
    let look = |vr: &mut Subsystems<MockBackend>, ov: &mut Overlay, at: Vec3, face: Vec3| {
        let facing = Quat::from_rotation_arc(Vec3::Z, face.normalize());
        ov.set_pose(watch, Pose::new(at, facing));
        ov.tick(vr);
        ov.gazed(watch)
    };
    // 頭（原点・正面 -Z）の少し下・手前で、面が頭を向いている。
    let raised = Vec3::new(0.05, -0.2, -0.35);
    assert!(look(&mut vr, &mut ov, raised, -raised));
    assert!(
        look(&mut vr, &mut ov, raised, Vec3::Y),
        "手首を回しきっていない（面が頭から 60° ほど上を向いている）"
    );
    assert!(
        !look(&mut vr, &mut ov, raised, Vec3::new(0.0, 1.0, -0.6)),
        "面が向こうへ倒れている（頭へ向いていない）"
    );
    let lowered = Vec3::new(0.2, -0.6, -0.1);
    assert!(
        !look(&mut vr, &mut ov, lowered, -lowered),
        "腕を下ろしている（視線から外れる）"
    );
    assert!(
        !look(&mut vr, &mut ov, Vec3::new(0.0, 0.0, -1.5), Vec3::Z),
        "遠い"
    );
    ov.set_hidden(&mut vr.backend, watch, true);
    assert!(!look(&mut vr, &mut ov, raised, -raised), "隠れている");
}
