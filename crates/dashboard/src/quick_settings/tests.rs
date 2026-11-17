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

/// `ms` 待ってから描き直す（時間で動くものを進める）。
fn redraw(cx: &mut VisualTestContext, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn height(cx: &mut VisualTestContext, selector: &'static str) -> f32 {
    f32::from(cx.debug_bounds(selector).unwrap().size.height)
}

/// 上に時刻、その下に届いた通知が新しい順に並ぶ（電池・テーマの切り替えはもう無い）。
#[gpui::test]
fn quick_settings_lists_notifications_newest_first(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![note(1), note(2), note(3)]);
    // 実際の画面の大きさ（`RES_PX`）で並べる。
    cx.simulate_resize(gpui::size(px(RES_PX.0 as f32), px(RES_PX.1 as f32)));
    cx.run_until_parked();
    let y = [
        "quick-time",
        "notification-3",
        "notification-2",
        "notification-1",
    ]
    .map(|s| cx.debug_bounds(s).unwrap().origin.y);
    assert!(y.windows(2).all(|w| w[0] < w[1]), "{y:?}");
    let (list, window) = (
        cx.debug_bounds("quick-settings").unwrap().size,
        cx.update(|window, _| window.viewport_size()),
    );
    assert!(
        list.height > window.height * 0.8 && list.width > window.width * 0.8,
        "一覧はパネルいっぱいに広がる: {list:?} / {window:?}"
    );
    assert!(cx.debug_bounds("notifications-empty").is_none());
    assert!(cx.debug_bounds("quick-theme").is_none());
    assert!(cx.debug_bounds("battery-hmd").is_none());
}

/// はまって出ている一覧に並んだ通知は読んだことになる（はまっていなければまだ）。
#[gpui::test]
fn the_shown_list_marks_its_notifications_read(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![note(1), note(2)]);
    let read = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| [1, 2].map(|id| ui::notification::is_read(id, cx)))
    };
    assert_eq!(read(cx), [false, false]);
    cx.update(|window, cx| {
        let c = cx.default_global::<PanelControl>();
        c.dock = Some((1, 0.5));
        c.panels.entry(1).or_default();
        ui::dock(2, cx);
        window.refresh();
    });
    cx.run_until_parked();
    assert_eq!(read(cx), [true, true]);
}

/// 通知が無ければ、空いた所の中央にチェックと「通知はありません」。
#[gpui::test]
fn quick_settings_says_when_there_are_no_notifications(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![]);
    cx.simulate_resize(gpui::size(px(RES_PX.0 as f32), px(RES_PX.1 as f32)));
    cx.run_until_parked();
    let empty = cx.debug_bounds("notifications-empty").unwrap();
    let (x, y) = (f32::from(empty.center().x), f32::from(empty.center().y));
    assert!(
        (x - RES_PX.0 as f32 / 2.0).abs() < 40.0,
        "横は中央: {empty:?}"
    );
    assert!(y > RES_PX.1 as f32 * 0.4, "下の空いた所の中ほど: {empty:?}");
}

/// 時刻の下に日付が来て、左端が揃う（横に並べると日付がベースラインから垂れていた）。
#[gpui::test]
fn the_date_sits_under_the_time(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![]);
    let (time, date) = (
        cx.debug_bounds("quick-time").unwrap(),
        cx.debug_bounds("quick-date").unwrap(),
    );
    assert!(date.top() >= time.bottom(), "{time:?} {date:?}");
    assert_eq!(date.left(), time.left());
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
    redraw(cx, design::animation::Motion::Return.duration_ms() + 50);
    redraw(cx, ui::swipe::COLLAPSE.duration_ms() + 50);
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

/// 広げる・たたむときは、高さが測った高さの間を滑らかに伸び縮みする（ボタンは広げ始めから並ぶ）。
#[gpui::test]
fn a_card_expands_and_collapses_smoothly(cx: &mut TestAppContext) {
    use design::animation::Motion;
    cx.update(|cx| cx.set_global(design::Motion::default()));
    let cx = quick_settings(cx, vec![note(1)]);
    let closed = height(cx, "notification-1");
    let at = cx.debug_bounds("notification-1").unwrap().center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("notification-action-1-0").is_some());
    let half = Motion::Morph.duration_ms() / 2;
    redraw(cx, half);
    let mid = height(cx, "notification-1");
    redraw(cx, half + 50);
    let open = height(cx, "notification-1");
    assert!(closed < mid && mid < open, "{closed} < {mid} < {open}");
    assert_eq!(mid, mid.round(), "画素に揃える");
    cx.simulate_mouse_move(gpui::point(px(1.0), px(1.0)), None, Modifiers::none());
    cx.run_until_parked();
    redraw(cx, Motion::Morph.duration_ms() / 2);
    let mid = height(cx, "notification-1");
    assert!(closed < mid && mid < open, "{closed} < {mid} < {open}");
    redraw(cx, Motion::Morph.duration_ms() / 2 + 50);
    assert_eq!(height(cx, "notification-1"), closed);
    assert!(cx.debug_bounds("notification-action-1-0").is_none());
}

/// 「すべて消去」で、上から順にスワイプしたように横へ抜けて畳まれ、OS の通知センターから消える（押したことにはしない）。
#[gpui::test]
fn clearing_all_removes_every_notification(cx: &mut TestAppContext) {
    let cx = quick_settings(cx, vec![note(1), note(2)]);
    let x = |cx: &mut VisualTestContext, s: &'static str| {
        f32::from(cx.debug_bounds(s).unwrap().origin.x)
    };
    let start = x(cx, "notification-2");
    click(cx, "notifications-clear");
    cx.run_until_parked();
    redraw(cx, ui::swipe::FLICK.duration_ms() / 3);
    let (top, below) = (x(cx, "notification-2"), x(cx, "notification-1"));
    assert!(
        top > below && top > start,
        "上から順に抜ける: {top} {below}"
    );
    for _ in 0..4 {
        redraw(cx, ui::swipe::COLLAPSE.duration_ms());
    }
    let mut removed: Vec<u32> = take_commands(cx, 2)
        .into_iter()
        .map(|c| match c {
            PanelCommand::RemoveNotification(id) => id,
            other => panic!("押したことにはしない: {other:?}"),
        })
        .collect();
    removed.sort();
    assert_eq!(removed, [1, 2]);
    assert!(cx.debug_bounds("notification-1").is_none());
    assert!(cx.debug_bounds("notifications-empty").is_some());
    assert!(
        cx.debug_bounds("notifications-clear").is_none(),
        "無ければ出さない"
    );
}

/// 通知が溜まっても、スクロールするのは一覧だけ（時刻と見出しは動かない）。
#[gpui::test]
fn only_the_list_scrolls(cx: &mut TestAppContext) {
    use gpui::{ScrollDelta, ScrollWheelEvent, point};
    let cx = quick_settings(cx, (1..=20).map(note).collect());
    cx.simulate_resize(gpui::size(px(RES_PX.0 as f32), px(RES_PX.1 as f32)));
    cx.run_until_parked();
    let y = |cx: &mut VisualTestContext, s: &'static str| cx.debug_bounds(s).unwrap().origin.y;
    let (time, clear, newest) = (
        y(cx, "quick-time"),
        y(cx, "notifications-clear"),
        y(cx, "notification-20"),
    );
    let list = cx.debug_bounds("notifications-list").unwrap();
    assert!(
        f32::from(list.bottom()) <= RES_PX.1 as f32,
        "一覧は画面に収まる"
    );
    cx.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-300.0))),
        ..Default::default()
    });
    cx.run_until_parked();
    assert_eq!(y(cx, "quick-time"), time);
    assert_eq!(y(cx, "notifications-clear"), clear);
    assert!(y(cx, "notification-20") < newest, "一覧はスクロールする");
}

/// 届いた時刻: 1 分以内は「今」、それから「n 分前」「n 時間前」「n 日前」、1 週間より古ければ日付と時刻。
#[gpui::test]
fn the_arrival_time_is_relative_then_absolute(cx: &mut TestAppContext) {
    use chrono::TimeZone;
    let now = chrono::Local
        .with_ymd_and_hms(2026, 11, 16, 15, 30, 0)
        .unwrap();
    let ago = |secs: u64| std::time::SystemTime::from(now) - Duration::from_secs(secs);
    cx.update(|cx| {
        cx.set_global(i18n::Lang::En);
        let s = |secs| stamp(cx, ago(secs), now).to_string();
        assert_eq!(s(20), "now");
        assert_eq!(s(5 * 60 + 20), "5 min ago");
        assert_eq!(s(59 * 60), "59 min ago");
        assert_eq!(s(2 * 3600 + 59 * 60), "2 h ago");
        assert_eq!(s(23 * 3600), "23 h ago");
        assert_eq!(s(24 * 3600), "1 d ago");
        assert_eq!(s(6 * 24 * 3600 + 23 * 3600), "6 d ago");
        assert_eq!(s(7 * 24 * 3600), "Nov 9, 15:30");
        assert_eq!(
            stamp(
                cx,
                std::time::SystemTime::from(now) + Duration::from_secs(30),
                now
            ),
            "now",
            "時計のずれで先の時刻"
        );
    });
    let cx = quick_settings(cx, vec![note(1)]);
    assert!(
        cx.debug_bounds("notification-time-1").is_some(),
        "カードに出る"
    );
}
