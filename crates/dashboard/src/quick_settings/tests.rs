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

/// トーストから開いたときは、その通知を広げて見せる（ボタンが並ぶ）。ほかを指すとそちらを広げ、前のものはたたむ。
/// 一覧を出しているのがこのパネルだと知らせる（トーストはそれが出ている間は出ない）。
#[gpui::test]
fn the_notification_opened_from_the_toast_is_expanded(cx: &mut TestAppContext) {
    cx.update(|cx| ui::notification::show_center(Some(2), cx));
    let cx = quick_settings(cx, vec![note(1), note(2)]);
    assert!(cx.debug_bounds("notification-action-2-0").is_some());
    assert!(cx.debug_bounds("notification-action-1-0").is_none());
    let center = cx.update(|_, cx| {
        let c = cx.global::<ui::notification::Center>();
        (c.panel, c.focus)
    });
    assert_eq!(center, (Some(2), None), "受け取った");
    let one = cx.debug_bounds("notification-1").unwrap().center();
    cx.simulate_mouse_move(one, None, Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("notification-action-1-0").is_some());
    assert!(cx.debug_bounds("notification-action-2-0").is_none());
}

/// 左右にスワイプすると、畳まれて下が詰まり、OS の通知センターから消す（押したことにはしない）。
#[gpui::test]
fn swiping_a_notification_away_removes_it(cx: &mut TestAppContext) {
    use gpui::{MouseButton, point};
    let cx = quick_settings(cx, vec![note(1), note(2)]);
    let card = cx.debug_bounds("notification-2").unwrap();
    let at = card.center();
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    for x in [0.2, 0.5, 0.8] {
        cx.simulate_mouse_move(
            at + point(card.size.width * x, px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
    }
    let end = at + point(card.size.width * 0.8, px(0.0));
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    let redraw = |cx: &mut VisualTestContext, ms: u64| {
        std::thread::sleep(Duration::from_millis(ms));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };
    redraw(cx, design::animation::Motion::Return.duration_ms() + 50);
    redraw(cx, design::animation::Motion::Shrink.duration_ms() + 50);
    redraw(cx, 0);
    assert!(cx.debug_bounds("notification-2").is_none());
    assert!(cx.debug_bounds("notification-1").is_some());
    assert_eq!(
        take_commands(cx, 2),
        [PanelCommand::RemoveNotification(2)],
        "押したことにはしない"
    );
}

/// 入っているアプリの通知には、広げると「（アプリ）を開く」が並ぶ（ID の大文字・小文字は問わない）。無いアプリには出さない。
#[gpui::test]
fn an_installed_app_can_be_opened_from_its_notification(cx: &mut TestAppContext) {
    let mut other = note(2);
    other.app.id = "Unknown".into();
    let cx = quick_settings(cx, vec![note(1), other]);
    cx.update(|_, cx| {
        let chat = InstalledApp {
            name: "Chat".into(),
            id: "chat".into(),
            exe: None,
            icon: None,
        };
        cx.set_global(crate::launcher::AppCatalog(vec![(chat, None)]));
    });
    for id in [1, 2] {
        let at = cx
            .debug_bounds(format!("notification-{id}").leak())
            .unwrap()
            .center();
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.run_until_parked();
        let open = cx.debug_bounds(format!("notification-open-{id}").leak());
        assert_eq!(open.is_some(), id == 1, "{id}");
    }
}
