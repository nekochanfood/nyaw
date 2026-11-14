use super::*;
use crate::test_util::*;
// `ui::commands`（親から）ではなくテストの道具の方。
use crate::test_util::commands;

/// アプリ一覧: 起動中のアプリとすべてのアプリを分けて出し（起動中でもすべてのアプリに出る）、起動中のものを
/// 見分ける。検索欄に打つと、どちらも名前に含むものだけになり、消すと戻る。
#[gpui::test]
fn apps_list_marks_running_apps_and_filters_by_search(cx: &mut TestAppContext) {
    let (list, cx) = cx.add_window_view(|_, cx| {
        let mut list = Screen::new(2, Page::Launcher, WeakEntity::new_invalid(), cx);
        (list.pending, list.apps_pending) = (None, false);
        list.sources = vec![(
            kernel::desktop::mock::source("Editor", SourceKind::Window),
            None,
        )];
        list.apps = ["Editor", "Paint"]
            .map(|name| {
                let app = InstalledApp {
                    name: name.into(),
                    id: name.into(),
                    exe: None,
                    icon: None,
                };
                (app, None)
            })
            .into();
        list
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("dashboard-running-0").is_some());
    assert!(
        cx.debug_bounds("dashboard-app-0").is_some()
            && cx.debug_bounds("dashboard-app-1").is_some()
    );
    list.read_with(cx, |l, _| {
        assert!(l.window_for(&l.apps[0].0).is_some(), "Editor は起動中");
        assert!(
            l.window_for(&l.apps[1].0).is_none(),
            "Paint は起動していない"
        );
    });
    let search = list.read_with(cx, |l, _| l.search.clone());
    cx.update(|window, cx| window.focus(&search, cx));
    cx.simulate_keystrokes("p a");
    cx.run_until_parked();
    assert_eq!(list.read_with(cx, |l, _| l.query.clone()), "pa");
    assert!(cx.debug_bounds("dashboard-running-0").is_none());
    assert!(cx.debug_bounds("dashboard-app-0").is_some());
    assert!(cx.debug_bounds("dashboard-app-1").is_none());
    click(cx, "dashboard-search-clear");
    cx.run_until_parked();
    assert!(list.read_with(cx, |l, _| l.query.is_empty()));
    assert!(cx.debug_bounds("dashboard-running-0").is_some());
}

/// ランチャーの描き直しの重さ（計測。常には流さない: `cargo test -p dashboard --lib launcher_redraw_cost -- --ignored --nocapture`）。
/// アプリ 450 個・ウィンドウ 30 個で、描き直し 1 回の平均を出す。
#[gpui::test]
#[ignore]
fn launcher_redraw_cost(cx: &mut TestAppContext) {
    let icon = || ui::render_image(64, 64, vec![200u8; 64 * 64 * 4]);
    let (_list, cx) = cx.add_window_view(|_, cx| {
        let mut list = Screen::new(2, Page::Launcher, WeakEntity::new_invalid(), cx);
        (list.pending, list.apps_pending) = (None, false);
        list.sources = (0..30)
            .map(|i| {
                (
                    kernel::desktop::mock::source(&format!("Window {i}"), SourceKind::Window),
                    icon(),
                )
            })
            .collect();
        list.apps = (0..450)
            .map(|i| {
                let app = InstalledApp {
                    name: format!("App {i}"),
                    id: format!("app-{i}"),
                    exe: (i % 2 == 0).then(|| format!("app{i}.exe")),
                    icon: None,
                };
                (app, icon())
            })
            .collect();
        list
    });
    cx.run_until_parked();
    let n = 30;
    let start = Instant::now();
    for _ in 0..n {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
    println!("launcher redraw: {:?} per frame", start.elapsed() / n);
}

/// 起動していないアプリを開くと、すぐその画面がはまって待ち（名前と「起動しています」）、ウィンドウが出たら
/// その画面で映し始めてバーに配信のボタンが増える。
#[gpui::test]
fn launching_app_opens_a_waiting_screen_that_starts_mirroring(cx: &mut TestAppContext) {
    let bar = cx
        .add_window(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        })
        .root(cx)
        .unwrap();
    let app = InstalledApp {
        name: "Editor".into(),
        id: "editor".into(),
        exe: Some("editor.exe".into()),
        icon: None,
    };
    let spec = launch_spec(app, None, Some(bar.downgrade()), 2, true);
    let nav_spec = spec.nav_bar_spec(4).unwrap();
    let (mirror, mcx) = cx.add_window_view(|window, cx| {
        Waiter(spec.build(4, window, cx).downcast::<Mirror>().unwrap())
    });
    mcx.run_until_parked();
    assert!(
        mcx.debug_bounds("mirror-waiting").is_some(),
        "待っている間の中身"
    );
    assert!(
        take_commands(mcx, 4).iter().any(docked_to_bar),
        "すぐはまる"
    );
    let mut window = kernel::desktop::mock::source("Editor - file.txt", SourceKind::Window);
    window.app = Some("Editor.exe".into());
    let inner = mirror.read_with(mcx, |w, _| w.0.clone());
    inner.update(mcx, |m, cx| m.found(window.clone(), cx));
    assert_eq!(
        take_commands(mcx, 4),
        [PanelCommand::Stream(Some(window), DEFAULT_FRAME_RATE)]
    );
    let streams = mcx.update(|_, cx| {
        bar.read(cx)
            .streams
            .iter()
            .map(|s| s.panel)
            .collect::<Vec<_>>()
    });
    assert_eq!(streams, [4], "バーに配信のボタンが増える");
    {
        let (_nav, ncx) =
            cx.add_window_view(|window, cx| ui::PanelRoot::new(nav_spec.build(5, window, cx)));
        ncx.update(|window, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .desktop = Some(Status::Streaming);
            cx.default_global::<PanelControl>()
                .panels
                .entry(5)
                .or_default()
                .status
                .pointed = true;
            window.refresh();
        });
        ncx.run_until_parked();
        assert!(ncx.debug_bounds("nav-minimize-source").is_some());
        click(ncx, "nav-minimize-source");
        assert_eq!(
            take_commands(ncx, 4),
            [PanelCommand::Hide(true), PanelCommand::Parent(None)]
        );
        assert!(bar.read_with(ncx, |d, _| d.streams[0].stashed));
    }
}

/// アプリの名前が長くても、タイル（押せる範囲）は列の幅のままで、名前は省略される。
#[gpui::test]
fn long_app_names_do_not_widen_tiles(cx: &mut TestAppContext) {
    const LONG_APP: &str =
        "A very very very very very very very very very very long application name";
    let (_list, cx) = cx.add_window_view(|_, cx| {
        let mut list = Screen::new(2, Page::Launcher, WeakEntity::new_invalid(), cx);
        (list.pending, list.apps_pending) = (None, false);
        list.apps = [LONG_APP.to_owned(), "Short".to_owned()]
            .map(|name| {
                let app = InstalledApp {
                    name: name.clone(),
                    id: name,
                    exe: None,
                    icon: None,
                };
                (app, None)
            })
            .into();
        list
    });
    cx.run_until_parked();
    let [long, short] = ["dashboard-app-0", "dashboard-app-1"].map(|s| cx.debug_bounds(s).unwrap());
    assert!(
        (long.size.width - short.size.width).abs() <= px(1.0),
        "{long:?} / {short:?}"
    );
    assert!(short.left() >= long.right(), "隣のタイルに重ならない");
    // タイルは列の幅（行の幅を列の数で割ったもの）いっぱい。中身の幅に縮まない。
    let width = cx.update(|window, _| window.viewport_size().width);
    assert!(
        long.size.width * APP_COLUMNS as f32 > width * 0.8,
        "{long:?} / {width:?}"
    );
    let window_id = cx.update(|window, _| window.window_handle().window_id());
    cx.simulate_mouse_move(long.center(), None, Modifiers::none());
    let tooltip = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            cx.global::<components::Tooltips>()
                .0
                .get(&(window_id, 1))
                .map(|(_, text, _)| text.to_string())
        })
    };
    assert_eq!(tooltip(cx).as_deref(), Some(LONG_APP));
    cx.simulate_mouse_move(short.center(), None, Modifiers::none());
    assert_eq!(tooltip(cx), None, "省略されないラベルには出さない");
}

/// 見出しの行は幅いっぱいで、見出しの文字は左、操作のボタンは右端に離れて並ぶ。
#[gpui::test]
fn section_buttons_sit_at_the_right_edge(cx: &mut TestAppContext) {
    let (_list, cx) = cx.add_window_view(|_, cx| {
        let mut list = Screen::new(2, Page::Launcher, WeakEntity::new_invalid(), cx);
        (list.pending, list.apps_pending) = (None, false);
        list
    });
    cx.run_until_parked();
    let width = cx.update(|window, _| window.viewport_size().width);
    for button in ["dashboard-refresh", "dashboard-nyapps-remove"] {
        if !nyapp::NORMAL_NYAPPS_ENABLED && button == "dashboard-nyapps-remove" {
            assert!(
                cx.debug_bounds(button).is_none(),
                "ロック中はニャプリの見出しを出さない"
            );
            continue;
        }
        let b = cx.debug_bounds(button).unwrap();
        assert!(
            b.right() > width * 0.9,
            "{button} は右端: {b:?} / {width:?}"
        );
    }
}

#[gpui::test]
fn choosing_a_source_docks_its_mirror_without_closing_the_dashboard(cx: &mut TestAppContext) {
    let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
    let bar = cx
        .add_window(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        })
        .root(cx)
        .unwrap();
    // バーにはまったランチャー（PanelId 2）。
    let (list, cx) = cx.add_window_view(|_, cx| {
        ui::dock(2, cx);
        let mut list = Screen::new(2, Page::Launcher, bar.downgrade(), cx);
        list.pending = None;
        list.sources = vec![(source.clone(), None)];
        list
    });
    let _ = list;
    cx.run_until_parked();
    click(cx, "dashboard-running-0");
    open_spawned(cx, 4);
    let c = commands(cx, 4);
    assert_eq!(c[0], PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE));
    assert!(docked_to_bar(&c[1]));
    // 一覧は入れ替わりに閉じ、ダッシュボードは開いたまま。バーに配信のボタンが増える。
    assert!(commands(cx, 2).contains(&PanelCommand::Close));
    assert!(!commands(cx, 1).contains(&PanelCommand::Close));
    let streams = cx.update(|_, cx| {
        bar.read(cx)
            .streams
            .iter()
            .map(|s| s.panel)
            .collect::<Vec<_>>()
    });
    assert_eq!(streams, [4]);
    assert_eq!(cx.update(|_, cx| ui::docked(cx)), Some(4));
}

/// ランチャーのニャプリのセクション: アイコンの無いものは頭文字の代わりのアイコンで並び、押すとランチャーと入れ替わりに
/// ニャプリのパネルがはまり、そのニャプリのものとして起動中の表に載る。
#[gpui::test]
fn launcher_lists_nyapps_and_docks_the_chosen_one(cx: &mut TestAppContext) {
    let bar = cx
        .add_window(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        })
        .root(cx)
        .unwrap();
    let (list, cx) = cx.add_window_view(|_, cx| {
        ui::dock(2, cx);
        let mut list = Screen::new(2, Page::Launcher, bar.downgrade(), cx);
        list.pending = None;
        list
    });
    cx.run_until_parked();
    let manifest = nyapp::Manifest::parse(
        "id = \"net.bettaworx.calc\"\nname = \"Calc\"\nversion = \"1\"\napi = 1\n",
    )
    .unwrap();
    list.update(cx, |l, cx| {
        // 本物のフォルダを読んだ結果を置き換える。
        l.nyapps = vec![nyapp::Installed {
            manifest,
            path: std::env::temp_dir().join("net.bettaworx.calc.nyapp"),
            icon: Default::default(),
        }];
        l.nyapps_pending = false;
        ui::notify(2, cx);
    });
    cx.run_until_parked();
    if !nyapp::NORMAL_NYAPPS_ENABLED {
        assert!(
            cx.debug_bounds("dashboard-nyapp-net.bettaworx.calc")
                .is_none(),
            "ロック中は入っているニャプリを並べない"
        );
        return;
    }
    assert!(
        cx.debug_bounds("nyapp-icon-fallback").is_some(),
        "アイコンが無ければ代わりを描く"
    );
    click(cx, "dashboard-nyapp-net.bettaworx.calc");
    open_spawned(cx, 4);
    assert!(commands(cx, 4).iter().any(docked_to_bar));
    assert!(
        commands(cx, 2).contains(&PanelCommand::Close),
        "ランチャーは入れ替わりに閉じる"
    );
    assert_eq!(
        cx.update(|_, cx| nyapp::panel_of("net.bettaworx.calc", cx)),
        Some(4)
    );
}

#[gpui::test]
fn launcher_lists_each_window_of_the_same_app(cx: &mut TestAppContext) {
    let (list, cx) = cx.add_window_view(|_, cx| {
        let mut list = Screen::new(2, Page::Launcher, WeakEntity::new_invalid(), cx);
        list.pending = None;
        list.apps = vec![(
            InstalledApp {
                id: "chrome".into(),
                name: "Chrome".into(),
                exe: Some("chrome.exe".into()),
                icon: None,
            },
            None,
        )];
        list.sources = ["Tab A", "Tab B", "Tab C"]
            .into_iter()
            .map(|title| {
                let mut source = kernel::desktop::mock::source(title, SourceKind::Window);
                source.app = Some("chrome.exe".into());
                (source, None)
            })
            .collect();
        list
    });
    let names = list.read_with(cx, |list, cx| {
        list.running_apps(cx)
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        names,
        ["Chrome · Tab A", "Chrome · Tab B", "Chrome · Tab C"]
    );
    list.update(cx, |list, _| {
        for (source, _) in &mut list.sources {
            source.name = "New Tab".into();
        }
    });
    let names = list.read_with(cx, |list, cx| {
        list.running_apps(cx)
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        names,
        [
            "Chrome · New Tab",
            "Chrome · New Tab (2)",
            "Chrome · New Tab (3)"
        ]
    );
}
