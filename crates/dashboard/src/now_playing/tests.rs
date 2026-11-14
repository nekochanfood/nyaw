use super::*;
use crate::test_util::*;
// `ui::commands`（親から）ではなくテストの道具の方。
use crate::test_util::commands;

/// 再生中の画面: 曲と位置・長さを出し、操作ボタンとシークバーはカーネルへの操作を積む。
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
    // 前へはできないので、押しても何も積まない。
    click(cx, "now-playing-previous");
    assert_eq!(
        commands(cx, 2),
        [
            PanelCommand::Media(media::Command::PlayPause),
            PanelCommand::Media(media::Command::Next),
        ]
    );
    // シークバーの右端近くを押すと、曲の終わり近くへ移る。
    let bar = cx.debug_bounds("now-playing-seek").unwrap();
    let at = gpui::point(bar.right() - px(1.0), bar.center().y);
    cx.simulate_click(at, Modifiers::none());
    let last = commands(cx, 2).pop();
    assert!(
        matches!(last, Some(PanelCommand::Media(media::Command::Seek(d))) if d > Duration::from_secs(190))
    );
}

/// メディアが 1 つなら切り替えを出さず、複数なら右上の前後のボタンで切り替えを積む。
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
    let c = commands(cx, 2);
    assert!(c.contains(&PanelCommand::Media(media::Command::Switch(false))));
    assert!(c.contains(&PanelCommand::Media(media::Command::Switch(true))));
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
