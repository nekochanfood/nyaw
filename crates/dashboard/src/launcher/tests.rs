use super::*;
use crate::test_util::*;
// `ui::commands`（親モジュール由来）ではなくテストユーティリティ側。
use crate::test_util::commands;

/// アプリ一覧: 起動中のアプリと全アプリを分離表示し（起動中でも全アプリ側にも表示される）、起動中のものを
/// 識別する。検索欄に入力すると両方とも名前に一致するもののみに絞られ、クリアで元に戻る。
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
        assert!(l.window_for(&l.apps[0].0).is_some(), "Editor は起動中のはず");
        assert!(
            l.window_for(&l.apps[1].0).is_none(),
            "Paint は未起動のはず"
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

/// ランチャーの再描画コスト計測（通常は実行しない: `cargo test -p dashboard --lib launcher_redraw_cost -- --ignored --nocapture`）。
/// アプリ 450 個・ウィンドウ 30 個の条件で、1 回の再描画にかかる平均時間を計測する。
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

/// 未起動のアプリを開くと、即座にドッキングして待機画面（名前と「起動しています」）を表示し、ウィンドウが出現したら
/// その画面で配信を開始してバーに配信ボタンが追加される。
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
        "待機中の画面内容が存在するはず"
    );
    assert!(
        take_commands(mcx, 4).iter().any(docked_to_bar),
        "即座にドッキングするはず"
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
    assert_eq!(streams, [4], "バーに配信ボタンが追加されるはず");
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

/// アプリ名が長くても、タイル（クリック可能な領域）は列幅を維持し、名前は省略表示される。
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
    assert!(short.left() >= long.right(), "隣のタイルと重ならないはず");
    // タイルは列幅（行幅を列数で除したもの）いっぱいに広がり、内容の幅に縮小しない。
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
    assert_eq!(tooltip(cx), None, "省略されないラベルにはツールチップを表示しない");
}

/// 見出し行は全幅を占め、見出しテキストは左寄せ、操作ボタンは右端に配置される。
#[gpui::test]
fn section_buttons_sit_at_the_right_edge(cx: &mut TestAppContext) {
    let (_list, cx) = cx.add_window_view(|_, cx| {
        let mut list = Screen::new(2, Page::Launcher, WeakEntity::new_invalid(), cx);
        (list.pending, list.apps_pending) = (None, false);
        list
    });
    cx.run_until_parked();
    let width = cx.update(|window, _| window.viewport_size().width);
    let b = cx.debug_bounds("dashboard-refresh").unwrap();
    assert!(
        b.right() > width * 0.9,
        "dashboard-refresh は右端: {b:?} / {width:?}"
    );
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
    // バーにドッキング済みのランチャー（PanelId 2）。
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
    // 一覧は入れ替わりにクローズされ、ダッシュボードは開いたまま。バーに配信ボタンが追加される。
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
