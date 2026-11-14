use super::*;
use crate::test_util::*;
// `ui::commands`（親から）ではなくテストの道具の方。
use crate::test_util::commands;

/// メニューボタンはバーに固定したポップオーバーを開き、その項目を選ぶと画面をはめてメニューを閉じる。
#[gpui::test]
fn menu_items_open_screens_and_close_the_menu(cx: &mut TestAppContext) {
    let bar = cx
        .add_window(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        })
        .root(cx)
        .unwrap();
    let (_bar_view, bcx) = cx.add_window_view({
        let bar = bar.clone();
        move |_, _| BarView(bar)
    });
    bcx.run_until_parked();
    click(bcx, "dashboard-menu");
    let spec = bcx.update(|_, cx| cx.default_global::<PanelControl>().spawns.pop().unwrap());
    assert!(
        spec.parent()
            .is_some_and(|p| p.panel == 1 && p.docked && p.dismiss)
    );
    let (_menu, mcx) = cx.add_window_view(|_, _| Menu {
        panel: 9,
        dashboard: bar.downgrade(),
    });
    mcx.run_until_parked();
    click(mcx, "menu-settings");
    assert!(commands(mcx, 9).contains(&PanelCommand::Close));
    assert_eq!(
        mcx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
        1
    );
}

/// 終了の確かめのダイアログで終了を選ぶとアプリを終え、キャンセルならダイアログだけ閉じる。
#[gpui::test]
fn quit_dialog_quits_or_cancels(cx: &mut TestAppContext) {
    let (_dialog, cx) = cx.add_window_view(|_, _| QuitDialog { panel: 7 });
    cx.run_until_parked();
    click(cx, "quit-cancel");
    assert!(commands(cx, 7).contains(&PanelCommand::Close));
    assert!(!cx.update(|_, cx| cx.global::<PanelControl>().quit));
    click(cx, "quit-confirm");
    assert!(cx.update(|_, cx| cx.global::<PanelControl>().quit));
}

/// メニューは、中央に寄ったバーの左端に揃えて開く（幅が狭いほど右にずれる）。
#[test]
fn menu_follows_the_bar_left_edge() {
    let full = menu_offset(components::PANEL_MARGIN).position.x;
    let narrow = menu_offset(components::PANEL_MARGIN + 100.0).position.x;
    assert!((narrow - full - 100.0 * BAR_M.x / BAR_PX.0 as f32).abs() < 1e-5);
}
