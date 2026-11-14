use super::*;
use crate::test_util::*;
// `ui::commands`（親から）ではなくテストの道具の方。
use crate::test_util::commands;

/// バーを `pose` に置くと、はめた画面は正面 `distance`・頭の高さから `height` 上に垂直に立つ。既定（今までの置き場所）は
/// 目の高さの正面 1.1m で、バーはその下で 1m 以上離れる。
#[test]
fn docked_screen_stands_upright_at_the_set_distance_and_height() {
    let b = docked_at(kernel::DASHBOARD_DISTANCE, 0.0);
    assert!(b.y < -0.3 && b.length() >= 1.0, "{b}");
    docked_at(kernel::DASHBOARD_DISTANCE_MAX, kernel::DASHBOARD_HEIGHT_MIN);
    docked_at(kernel::DASHBOARD_DISTANCE_MIN, kernel::DASHBOARD_HEIGHT_MAX);
}

/// バーを `pose(distance, height)` に置いて画面をはめ、画面がそこに垂直に立つことを確かめる。バーの位置を返す。
fn docked_at(distance: f32, height: f32) -> Vec3 {
    use kernel::overlay::{Overlay, Parent};
    use kernel::subsystems::Subsystems;
    use kernel::vr::mock::MockBackend;
    let mut vr = Subsystems::with_backend(MockBackend::default());
    let mut ov = Overlay::new();
    let bar = ov
        .add_panel(
            &mut vr.backend,
            "bar",
            pose(distance, height),
            BAR_M,
            BAR_PX,
        )
        .unwrap();
    let screen = ov
        .add_panel(&mut vr.backend, "screen", SCREEN_POSE, SIZE_M, RES_PX)
        .unwrap();
    let nav = ov
        .add_panel(
            &mut vr.backend,
            "nav",
            Pose::IDENTITY,
            Vec2::new(1.0, ui::NAV_BAR_HEIGHT_M),
            (100, 10),
        )
        .unwrap();
    ov.set_nav_bar(&mut vr.backend, nav, screen);
    // 出入りのアニメーションの途中は小さく見えるので、出きった状態で測る。
    ov.set_animated(&mut vr.backend, screen, false);
    ov.set_parent(
        &mut vr.backend,
        screen,
        Some(Parent {
            panel: bar,
            carry: true,
            docked: true,
            slot: Some(BAR_TILT),
            ..Default::default()
        }),
    );
    for _ in 0..3 {
        ov.tick(&mut vr);
    }
    let s = ov.panel(screen).pose;
    let want = Vec3::new(0.0, height, -distance);
    assert!(
        s.position.abs_diff_eq(want, 1e-3),
        "{} != {want}",
        s.position
    );
    assert!(s.orientation.abs_diff_eq(Quat::IDENTITY, 1e-4));
    ov.panel(bar).pose.position
}

#[gpui::test]
fn opening_shows_nothing_and_buttons_toggle_docked_screens(cx: &mut TestAppContext) {
    // バー（PanelId 1）の上を、画面をはめる場所にする。
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
        0
    );
    // 設定を選ぶと（メニューから。`menu_items_open_screens_and_close_the_menu`）、main が渡した設定画面がバーの上にはまる。
    bar.update(cx, |d, cx| d.toggle(Page::Settings, cx));
    open_spawned(cx, 2);
    assert!(commands(cx, 2).iter().any(docked_to_bar));
    assert_eq!(
        cx.update(|_, cx| bar.read(cx).current(cx)),
        Some(Page::Settings)
    );
    // クイック設定を押すと、設定と入れ替わる。
    click(cx, "dashboard-quick-settings");
    open_spawned(cx, 3);
    assert!(commands(cx, 2).contains(&PanelCommand::Close));
    assert_eq!(
        cx.update(|_, cx| bar.read(cx).current(cx)),
        Some(Page::QuickSettings)
    );
    // もう一度押すと閉じる。
    click(cx, "dashboard-quick-settings");
    assert!(commands(cx, 3).contains(&PanelCommand::Close));
    assert_eq!(cx.update(|_, cx| bar.read(cx).current(cx)), None);
}

/// 設定を開いた回数（[`update_button_shows_only_with_a_new_release_and_opens_settings`] だけが使う）。
/// 起動時に開く画面の配信（本物の OS の一覧）も `spawns` に積まれるので、要求の数では数えない。
static SETTINGS_OPENED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn counted_settings(from: PanelId, opened: Opened) -> PanelSpec {
    SETTINGS_OPENED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    summon(from, opened)
}

/// 新しいリリースがあるときだけ、ランチャーの右に強調色のアップデートのボタンが出る。押すと設定をはめ、
/// 設定にアップデートを出すよう頼む。設定がもうはまっていれば閉じずに、その画面を描き直させる。
#[gpui::test]
fn update_button_shows_only_with_a_new_release_and_opens_settings(cx: &mut TestAppContext) {
    let opened = || SETTINGS_OPENED.load(std::sync::atomic::Ordering::SeqCst);
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, counted_settings, summon, cx)
    });
    redraw_bar(cx);
    assert!(
        cx.debug_bounds("dashboard-update").is_none(),
        "最新なら出ない"
    );
    cx.update(|_, cx| {
        cx.default_global::<updater::Updater>().found = vec![updater::Release {
            version: "2026.0.1-patch.2".into(),
            build: u32::MAX,
            title: "v2026.0.1-patch.2".into(),
            notes: "## What's Changed".into(),
            published_at: None,
            page: "https://github.com/nekochanfood/nyaw/releases".into(),
            prerelease: false,
            installer: updater::Installer {
                name: "nyaw-2026.0.1-patch.2-setup.exe".into(),
                url: "https://example.com/i.exe".into(),
                size: 1,
                sha256: "0".repeat(64),
            },
        }]
    });
    redraw_bar(cx);
    let x = |cx: &mut VisualTestContext, s: &'static str| cx.debug_bounds(s).unwrap().origin.x;
    assert!(x(cx, "dashboard-launcher") < x(cx, "dashboard-update"));
    click(cx, "dashboard-update");
    assert_eq!(opened(), 1, "設定をはめる");
    assert!(
        cx.update(|_, cx| updater::take_reveal(cx)),
        "アップデートを出すよう頼む"
    );
    // 設定（パネル 7）がはまっている間に押すと、閉じも開き直しもせず、描き直させるだけ。
    bar.update(cx, |d, cx| d.track(Page::Settings, 7, cx));
    cx.update(|_, cx| ui::dock(7, cx));
    redraw_bar(cx);
    click(cx, "dashboard-update");
    assert_eq!(opened(), 1, "開き直さない");
    assert!(!commands(cx, 7).contains(&PanelCommand::Close));
    assert!(cx.update(|_, cx| cx.global::<PanelControl>().panels[&7].redraw));
    assert!(cx.update(|_, cx| updater::take_reveal(cx)));
    // リリースが消えれば（入れ替えた後など）、ボタンも消える。
    cx.update(|_, cx| cx.default_global::<updater::Updater>().found.clear());
    redraw_bar(cx);
    assert!(cx.debug_bounds("dashboard-update").is_none());
}

/// 何か再生しているときだけ、バーに再生中の画面のボタンが出る。
#[gpui::test]
fn now_playing_button_shows_only_while_something_plays(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| Dashboard::new(1, summon, summon, cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("dashboard-now-playing").is_none());
    cx.update(|_, cx| cx.default_global::<PanelControl>().media = Some(playing()));
    bar.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(cx.debug_bounds("dashboard-now-playing").is_some());
}

/// 進行中の作業の知らせは、バーのクイック設定と再生中の間に出る。押すと出したニャプリがはまり、
/// 終わったカウントダウンの知らせは片付く。
#[gpui::test]
fn activities_sit_between_quick_settings_and_now_playing(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        cx.default_global::<PanelControl>().media = Some(playing());
        Dashboard::new(1, summon, summon, cx)
    });
    let manifest = nyapp::Manifest::parse(
        "id = \"net.bettaworx.clock\"\nname = \"Clock\"\nversion = \"1\"\napi = 1\n",
    )
    .unwrap();
    let clock = nyapp::Installed {
        manifest,
        path: std::env::temp_dir().join("net.bettaworx.clock.nyapp"),
        icon: Default::default(),
    };
    cx.update(|_, cx| {
        let ends = nyapp::activity::now_ms() + 90_000;
        let countdown = nyapp::Activity::Countdown {
            title: "Timer".into(),
            ends_at_ms: ends,
            total_ms: 120_000,
        };
        nyapp::activity::set(&clock, Some(countdown), cx);
    });
    redraw_bar(cx);
    let x = |cx: &mut VisualTestContext, s: &'static str| cx.debug_bounds(s).unwrap().origin.x;
    let (quick, activity, playing) = (
        x(cx, "dashboard-quick-settings"),
        x(cx, "dashboard-activity-net.bettaworx.clock"),
        x(cx, "dashboard-now-playing"),
    );
    let pill = cx
        .debug_bounds("dashboard-activity-net.bettaworx.clock")
        .unwrap();
    assert_eq!(pill.size.width, pill.size.height, "知らせは 1:1");
    assert!(
        quick < activity && activity < playing,
        "{quick:?} < {activity:?} < {playing:?}"
    );
    // 終わったものを押すと片付き、ニャプリがはまる。
    cx.update(|_, cx| {
        let live = nyapp::activity::all(cx)[0].clone();
        nyapp::activity::set(&live.app, None, cx);
        let ends = nyapp::activity::now_ms() - 1;
        let done = nyapp::Activity::Countdown {
            title: "Timer".into(),
            ends_at_ms: ends,
            total_ms: 1,
        };
        nyapp::activity::set(&live.app, Some(done), cx);
    });
    cx.run_until_parked();
    assert!(cx.update(|_, cx| nyapp::activity::all(cx)[0].finished));
    redraw_bar(cx);
    click(cx, "dashboard-activity-net.bettaworx.clock");
    assert!(
        cx.update(|_, cx| nyapp::activity::all(cx).is_empty()),
        "終わったものは片付く"
    );
    open_spawned(cx, 4);
    assert!(
        commands(cx, 4).iter().any(docked_to_bar),
        "ニャプリがはまる"
    );
}

/// 出ている間に何も無い所を押すと隠す（開いた時点までの数では隠さない）。
#[gpui::test]
fn pressing_outside_hides_the_dashboard(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| Dashboard::new(1, summon, summon, cx));
    let press = |cx: &mut VisualTestContext, n| {
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(1)
                .or_default()
                .status
                .outside_press = n
        });
        cx.run_until_parked();
    };
    press(cx, 4);
    assert!(!commands(cx, 1).contains(&PanelCommand::Hide(true)));
    press(cx, 5);
    assert!(commands(cx, 1).contains(&PanelCommand::Hide(true)));
}

/// 隠している間に押した分は、出し直したのと同じ状態の書き戻しで届いても隠さない（開く音だけ鳴って出ない）。
#[gpui::test]
fn press_while_hidden_does_not_hide_the_reshown_dashboard(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| Dashboard::new(1, summon, summon, cx));
    let set = |cx: &mut VisualTestContext, n, hidden| {
        cx.update(|_, cx| {
            let status = &mut cx
                .default_global::<PanelControl>()
                .panels
                .entry(1)
                .or_default()
                .status;
            (status.outside_press, status.hidden) = (n, hidden);
        });
        cx.run_until_parked();
    };
    set(cx, 4, true);
    // 出し直したのと同じフレームに、何も無い所を押した数も増えた。
    set(cx, 5, false);
    assert!(!commands(cx, 1).contains(&PanelCommand::Hide(true)));
    set(cx, 6, false);
    assert!(commands(cx, 1).contains(&PanelCommand::Hide(true)));
}

/// ダッシュボードを出し直すと、はまっている画面（配信なら、そのウィンドウ）を前に出すよう頼む。
#[gpui::test]
fn showing_the_dashboard_focuses_the_docked_stream(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    cx.update(|_, cx| ui::dock(4, cx));
    take_commands(cx, 4);
    cx.update(|_, cx| show(1, cx));
    assert_eq!(take_commands(cx, 4), [PanelCommand::FocusSource]);
}

/// 滑り込みの間の幅の計算（クイック設定の自然な幅）が、出きったときの実際の幅と合う。合わないと出きる瞬間に飛ぶ。
#[gpui::test]
fn quick_settings_width_matches_the_laid_out_button(cx: &mut TestAppContext) {
    for with_battery in [false, true] {
        let (_bar, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().batteries = if with_battery {
                vec![battery(0.9, false)]
            } else {
                vec![]
            };
            Dashboard::new(1, summon, summon, cx)
        });
        cx.run_until_parked();
        let time = cx.update(|window, _| clock_width(window, font_size::MD));
        let actual = f32::from(
            cx.debug_bounds("dashboard-quick-settings")
                .unwrap()
                .size
                .width,
        );
        let expected = quick_settings_width(f32::from(time), with_battery);
        assert!(
            (actual - expected).abs() <= 1.0,
            "電池 {with_battery}: {actual} / {expected}"
        );
    }
}

/// 配信が溜まって最大幅を超えそうでも、バーはそこで頭打ちになり、横にスクロールできる。
#[gpui::test]
fn bar_stops_at_the_max_width_and_scrolls(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    bar.update(cx, |d, cx| {
        for id in 4..16 {
            d.add_stream(
                id,
                SourceKind::Window,
                format!("W{id}").into(),
                None,
                false,
                cx,
            );
        }
    });
    cx.run_until_parked();
    let width = f32::from(cx.debug_bounds("dashboard-bar").unwrap().size.width);
    assert!(width <= MAX_BAR_PX + 1.0, "頭打ち: {width}");
    assert!(bar.read_with(cx, |d, _| d.scroll.max_offset().x) > px(0.0));
}

/// バーは窓いっぱいに広がらず、中身の幅に収まって中央に置かれる。
#[gpui::test]
fn bar_fits_its_contents_and_is_centered(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| Dashboard::new(1, summon, summon, cx));
    cx.run_until_parked();
    let width = cx.update(|window, _| window.viewport_size().width);
    let (first, last) = (
        cx.debug_bounds("dashboard-menu").unwrap(),
        cx.debug_bounds("dashboard-launcher").unwrap(),
    );
    let content = last.right() - first.left();
    assert!(
        content < width * 0.8,
        "窓いっぱいに広がらない: {content:?} / {width:?}"
    );
    let (left, right) = (first.left(), width - last.right());
    assert!(
        (left - right).abs() <= px(2.0),
        "中央に置く: {left:?} / {right:?}"
    );
}

/// 開いた配信はバーに溜まり、押すとはまる。はまっていた配信は閉じずに隠して外し、もう一度押すと隠す。
/// 閉じた配信のボタンは消える。
#[gpui::test]
fn streams_accumulate_and_switch_without_closing(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    bar.update(cx, |d, cx| {
        d.add_stream(4, SourceKind::Screen, "Display 1".into(), None, false, cx);
        d.add_stream(5, SourceKind::Window, "Editor".into(), None, false, cx);
    });
    cx.update(|_, cx| ui::dock(4, cx));
    cx.run_until_parked();
    // 配信は区切り線の右に、押せるボタンとして並ぶ。
    assert!(cx.debug_bounds("dashboard-stream-4").is_some());
    click(cx, "dashboard-stream-5");
    assert_eq!(
        commands(cx, 4)[1..],
        [PanelCommand::Hide(true), PanelCommand::Parent(None)]
    );
    // 隠したままはめてから出す。
    let five = commands(cx, 5);
    assert!(docked_to_bar(&five[0]));
    assert_eq!(five[1], PanelCommand::FitHeight(SIZE_M.y));
    assert_eq!(
        five[3],
        PanelCommand::FocusSource,
        "出したウィンドウを前に出す"
    );
    assert!(
        !commands(cx, 4).contains(&PanelCommand::Close),
        "隠すだけで閉じない"
    );
    // もう一度押すと隠す。
    click(cx, "dashboard-stream-5");
    assert!(commands(cx, 5).contains(&PanelCommand::Hide(true)));
    // 閉じた配信のボタンは消える。
    cx.update(|_, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(4)
            .or_default()
            .status
            .closed = true
    });
    bar.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let streams = cx.update(|_, cx| {
        bar.read(cx)
            .streams
            .iter()
            .map(|s| s.panel)
            .collect::<Vec<_>>()
    });
    assert_eq!(streams, [5]);
}

/// 自分で外して独立させた配信のボタンは残り、閉じたときだけ消える。押すとダッシュボードに戻る。
#[gpui::test]
fn undocked_stream_keeps_its_button_until_closed(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    bar.update(cx, |d, cx| {
        d.add_stream(4, SourceKind::Window, "Editor".into(), None, false, cx)
    });
    let status = |cx: &mut VisualTestContext, parent, closed| {
        cx.update(|_, cx| {
            let s = &mut cx
                .default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status;
            (s.parent, s.closed) = (parent, closed);
        });
        bar.update(cx, |d, cx| d.prune_streams(cx));
    };
    status(cx, Some(1), false);
    status(cx, None, false);
    assert_eq!(bar.read_with(cx, |d, _| d.streams.len()), 1);
    bar.update(cx, |d, cx| d.toggle_stream(4, cx));
    assert!(take_commands(cx, 4).contains(&PanelCommand::Hide(false)));
    status(cx, None, true);
    assert_eq!(bar.read_with(cx, |d, _| d.streams.len()), 0);
}

/// 常駐の配信は、独立させてもバーに残り、しまうと閉じずに隠れる。
#[gpui::test]
fn resident_streams_stay_on_the_bar_and_minimize(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    bar.update(cx, |d, cx| {
        d.add_stream(4, SourceKind::Screen, "Display 1".into(), None, true, cx)
    });
    // はめてから自分で外した（独立させた）。
    let set_parent = |cx: &mut VisualTestContext, parent| {
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .parent = parent
        });
        bar.update(cx, |d, cx| d.prune_streams(cx));
    };
    set_parent(cx, Some(1));
    set_parent(cx, None);
    assert_eq!(
        bar.read_with(cx, |d, _| d.streams.len()),
        1,
        "独立させても残る"
    );
    bar.update(cx, |d, cx| d.minimize(4, cx));
    let c = commands(cx, 4);
    assert!(c.contains(&PanelCommand::Hide(true)));
    assert!(!c.contains(&PanelCommand::Close), "閉じない");
    assert!(bar.read_with(cx, |d, _| d.streams[0].stashed));
}
