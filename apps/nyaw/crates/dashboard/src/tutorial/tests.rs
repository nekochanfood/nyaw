use super::*;

/// 手順は開始画面に始まり完了で終わり、`next` は [`STEPS`] の順に進む（完了の次は完了のまま）。
#[test]
fn steps_go_from_the_intro_to_done_in_order() {
    assert_eq!(STEPS.first(), Some(&Step::Intro));
    assert_eq!(STEPS.last(), Some(&Step::Done));
    for pair in STEPS.windows(2) {
        assert_eq!(pair[0].next(), pair[1]);
    }
    assert_eq!(Step::Done.next(), Step::Done);
}

/// 案内のパネルは、はめた画面の右端から離して画面の中心と同じ高さに立ち、頭のほうへ向く（距離・高さの設定によらない）。
#[test]
fn the_coach_stands_right_of_the_docked_screen_facing_the_head() {
    let settings = [
        (kernel::DASHBOARD_DISTANCE, 0.0),
        (kernel::DASHBOARD_DISTANCE_MIN, kernel::DASHBOARD_HEIGHT_MAX),
        (kernel::DASHBOARD_DISTANCE_MAX, kernel::DASHBOARD_HEIGHT_MIN),
    ];
    for (distance, height) in settings {
        let coach = pose(distance, height) * coach_offset(distance, height);
        let left = coach.transform_point(Vec3::new(-COACH_M.x / 2.0, 0.0, 0.0));
        assert!(
            (left.x - (SIZE_M.x / 2.0 + COACH_GAP)).abs() < 1e-4,
            "{left}"
        );
        assert!((left.z + distance).abs() < 1e-4, "{left}");
        assert!((coach.position.y - (HEIGHT_ORIGIN + height)).abs() < 1e-4);
        // 画面と同じく立てて置き、水平には頭のほうへ向ける。
        let facing = coach.orientation * Vec3::Z;
        assert!(facing.y.abs() < 1e-4, "{facing}");
        let head = Vec3::new(-coach.position.x, 0.0, -coach.position.z).normalize();
        assert!(facing.dot(head) > 0.8, "{facing}");
    }
}
