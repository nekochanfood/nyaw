use super::*;
use crate::test_util::*;
// `ui::commands`（親モジュール由来）ではなくテストユーティリティ側。
use crate::test_util::commands;

/// 再生中画面: 曲名・再生位置・長さを表示し、操作ボタンとシークバーはカーネルへのコマンドを発行する。
#[gpui::test]
fn now_playing_screen_shows_the_song_and_sends_commands(cx: &mut TestAppContext) {
    let (_screen, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().media = Some(playing());
        Screen::new(2, Page::NowPlaying, WeakEntity::new_invalid(), cx)
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("now-playing-title").is_some());
    assert!(cx.debug_bounds("now-playing-position").is_some());
    click(cx, "now-playing-play-pause");
    click(cx, "now-playing-next");
    // 前へは無効のため、押下してもコマンドは発行されない。
    click(cx, "now-playing-previous");
    assert_eq!(
        take_requests(cx),
        [
            Request::Media(media::Command::PlayPause),
            Request::Media(media::Command::Next),
        ]
    );
    // シークバーの右端付近を押下すると、曲の終盤付近へシークする。
    let bar = cx.debug_bounds("now-playing-seek").unwrap();
    let at = gpui::point(bar.right() - px(1.0), bar.center().y);
    cx.simulate_click(at, Modifiers::none());
    let last = take_requests(cx).pop();
    assert!(
        matches!(last, Some(Request::Media(media::Command::Seek(d))) if d > Duration::from_secs(190))
    );
}

/// メディアセッションが 1 つなら切り替えボタンを非表示にし、複数なら右上の前後ボタンで切り替えコマンドを発行する。
#[gpui::test]
fn media_switch_shows_only_with_several_sessions(cx: &mut TestAppContext) {
    let (screen, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().media = Some(playing());
        Screen::new(2, Page::NowPlaying, WeakEntity::new_invalid(), cx)
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("now-playing-next-media").is_none());
    cx.update(|_, cx| {
        let mut media = (*playing()).clone();
        media.sessions = 2;
        cx.default_global::<PanelControl>().media = Some(Arc::new(media));
    });
    screen.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    click(cx, "now-playing-previous-media");
    click(cx, "now-playing-next-media");
    let c = take_requests(cx);
    assert!(c.contains(&Request::Media(media::Command::Switch(false))));
    assert!(c.contains(&Request::Media(media::Command::Switch(true))));
}

#[gpui::test]
fn unavailable_app_window_is_not_clickable(cx: &mut TestAppContext) {
    let (screen, cx) = cx.add_window_view(|_, cx| {
        let mut media = (*playing()).clone();
        media.app.id = "nyaw-unavailable-test.exe".into();
        media.app.name = "nyaw-unavailable-test".into();
        cx.default_global::<PanelControl>().media = Some(Arc::new(media));
        Screen::new(2, Page::NowPlaying, WeakEntity::new_invalid(), cx)
    });
    cx.run_until_parked();
    click(cx, "now-playing-app");
    assert!(!screen.read_with(cx, |s, _| s.open_app));
    assert!(commands(cx, 2).is_empty());
}
