use super::*;
use crate::test_util::*;

fn quick_settings(cx: &mut TestAppContext, notes: Vec<Notification>) -> &mut VisualTestContext {
    let (_screen, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().notifications =
            notes.into_iter().map(Arc::new).collect();
        Screen::new(2, Page::QuickSettings, WeakEntity::new_invalid(), cx)
    });
    cx.run_until_parked();
    cx
}

/// 上に時刻、その下に届いた通知が新しい順に並ぶ（電池・テーマの切り替えはもう無い）。
#[gpui::test]
fn quick_settings_lists_notifications_newest_first(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![note(1), note(2), note(3)]);
    let y = [
        "quick-time",
        "notification-3",
        "notification-2",
        "notification-1",
    ]
    .map(|s| cx.debug_bounds(s).unwrap().origin.y);
    assert!(y.windows(2).all(|w| w[0] < w[1]), "{y:?}");
    assert!(cx.debug_bounds("notifications-empty").is_none());
    assert!(cx.debug_bounds("quick-theme").is_none());
    assert!(cx.debug_bounds("battery-hmd").is_none());
}

#[gpui::test]
fn quick_settings_says_when_there_are_no_notifications(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![]);
    assert!(cx.debug_bounds("notifications-empty").is_some());
}

/// ホバーすると通知のボタンが出て、押すとアプリへ届く（本文を押しても届く）。
#[gpui::test]
fn a_notification_in_the_list_opens_from_its_card_or_its_buttons(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![note(1)]);
    let take = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let state = cx.default_global::<PanelControl>().panels.entry(2);
            std::mem::take(&mut state.or_default().commands)
        })
    };
    assert!(cx.debug_bounds("notification-action-1-0").is_none());
    let card = cx.debug_bounds("notification-1").unwrap().center();
    cx.simulate_mouse_move(card, None, Modifiers::none());
    cx.run_until_parked();
    click(cx, "notification-action-1-0");
    cx.run_until_parked();
    assert_eq!(
        take(cx),
        [PanelCommand::Notification {
            id: 1,
            action: Some(0)
        }]
    );
    click(cx, "notification-1");
    cx.run_until_parked();
    assert_eq!(
        take(cx),
        [PanelCommand::Notification {
            id: 1,
            action: None
        }]
    );
}
