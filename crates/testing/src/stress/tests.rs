use super::*;
use crate::test_util::*;

/// Fill は上限ちょうどまで出し（待っている分も数える）、Grid・Close all は出したもののうち開いているものだけに効く。
#[gpui::test]
fn stress_fill_stops_at_limit_and_commands_reach_spawned(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| StressTest::new(0));
    cx.update(|_, cx| {
        let c = cx.default_global::<PanelControl>();
        (c.open, c.limit) = (10, 15);
    });
    cx.run_until_parked();
    let fill = cx.debug_bounds("stress-fill").unwrap().center();
    cx.simulate_click(fill, Modifiers::none());
    cx.simulate_click(fill, Modifiers::none());
    let spawns = cx.update(|_, cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns));
    assert_eq!(spawns.len(), 5);

    // 開いたことにする（ビルドで積まれる ID の代わり）。1 枚は閉じられた。
    view.update(cx, |s, _| s.spawned.borrow_mut().extend([1, 2, 3]));
    cx.update(|_, cx| {
        let c = cx.default_global::<PanelControl>();
        c.panels.entry(2).or_default().status.closed = true;
    });
    for id in ["stress-grid", "stress-close"] {
        let c = cx.debug_bounds(id).unwrap().center();
        cx.simulate_click(c, Modifiers::none());
    }
    let commands = cx.update(|_, cx| {
        let c = cx.global::<PanelControl>();
        [1, 2, 3].map(|id| c.panels.get(&id).map_or(0, |s| s.commands.len()))
    });
    assert_eq!(commands, [2, 0, 2]);
}

#[test]
fn grid_rows_are_full_before_wrapping() {
    assert_eq!(grid_pose(0).position.y, grid_pose(GRID_COLS - 1).position.y);
    assert!(grid_pose(GRID_COLS).position.y < grid_pose(0).position.y);
    assert!(grid_pose(0).position.x < 0.0 && grid_pose(GRID_COLS - 1).position.x > 0.0);
}
