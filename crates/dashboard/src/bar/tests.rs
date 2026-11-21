use super::*;
use crate::test_util::*;
// `ui::commands`（親モジュール由来）ではなくテストユーティリティ側。
use crate::test_util::commands;

/// バーを `pose` に配置すると、ドッキング画面は正面 `distance`・頭の高さから [`HEIGHT_ORIGIN`] + `height` 上に垂直に立つ。
/// 既定値は目の高さよりやや下の正面 1.3m で、バーはその下方かつ 1m 以上の距離に位置する。
#[test]
fn docked_screen_stands_upright_at_the_set_distance_and_height() {
    let b = docked_at(kernel::DASHBOARD_DISTANCE, 0.0);
    assert!(b.y < -0.3 && b.length() >= 1.0, "{b}");
    docked_at(kernel::DASHBOARD_DISTANCE_MAX, kernel::DASHBOARD_HEIGHT_MIN);
    docked_at(kernel::DASHBOARD_DISTANCE_MIN, kernel::DASHBOARD_HEIGHT_MAX);
}

/// バーを `pose(distance, height)` に配置して画面をドッキングし、画面がその位置に垂直に立つことを検証する。バーの位置を返す。
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
    // 出入りアニメーション中はスケールが小さいため、アニメーション完了状態で計測する。
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
    let want = Vec3::new(0.0, HEIGHT_ORIGIN + height, -distance);
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
    // バー（PanelId 1）の上をドッキングスロットとして設定する。
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
        0
    );
    // 設定を選択すると（メニューから。`menu_items_open_screens_and_close_the_menu`）、main が渡した設定画面がバー上にドッキングする。
    bar.update(cx, |d, cx| d.toggle(Page::Settings, cx));
    open_spawned(cx, 2);
    assert!(commands(cx, 2).iter().any(docked_to_bar));
    assert_eq!(
        cx.update(|_, cx| bar.read(cx).current(cx)),
        Some(Page::Settings)
    );
    // クイック設定を押下すると、設定と入れ替わる。
    click(cx, "dashboard-quick-settings");
    open_spawned(cx, 3);
    assert!(commands(cx, 2).contains(&PanelCommand::Close));
    assert_eq!(
        cx.update(|_, cx| bar.read(cx).current(cx)),
        Some(Page::QuickSettings)
    );
    // 再度押下するとクローズする。
    click(cx, "dashboard-quick-settings");
    assert!(commands(cx, 3).contains(&PanelCommand::Close));
    assert_eq!(cx.update(|_, cx| bar.read(cx).current(cx)), None);
}

/// 取り外して独立させた画面のボタンを押下しても、新規に開かずに再ドッキングする。クローズ済みなら新規に開く。
#[gpui::test]
fn undocked_screen_is_docked_again_instead_of_opened_twice(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    cx.run_until_parked();
    let spawns =
        |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<PanelControl>().spawns.len());
    bar.update(cx, |d, cx| d.toggle(Page::Settings, cx));
    open_spawned(cx, 2);
    // ナビゲーションバーの「…」の `nav-undock` と同様に取り外す。
    cx.update(|_, cx| ui::undock(2, cx));
    take_commands(cx, 2);
    assert_eq!(cx.update(|_, cx| bar.read(cx).current(cx)), None);
    bar.update(cx, |d, cx| d.toggle(Page::Settings, cx));
    assert_eq!(spawns(cx), 0, "新規に開かないはず");
    assert!(take_commands(cx, 2).iter().any(docked_to_bar));
    assert_eq!(
        cx.update(|_, cx| bar.read(cx).current(cx)),
        Some(Page::Settings)
    );
    // クローズ済みなら新規に開く。
    cx.update(|_, cx| {
        let ctl = cx.default_global::<PanelControl>();
        ctl.panels.entry(2).or_default().status.closed = true;
    });
    bar.update(cx, |d, cx| d.toggle(Page::Settings, cx));
    assert_eq!(spawns(cx), 1);
}

/// 設定を開いた回数（[`update_button_shows_only_with_a_new_release_and_opens_settings`] 専用）。
/// 起動時に開くモニター配信（実際の OS 一覧）も `spawns` に積まれるため、スポーン要求数では計測しない。
static SETTINGS_OPENED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn counted_settings(from: PanelId, opened: Opened) -> PanelSpec {
    SETTINGS_OPENED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    summon(from, opened)
}

/// 新規リリースがある場合のみ、ランチャーの右に強調色のアップデートボタンが表示される。押下すると設定をドッキングし、
/// 設定にアップデート画面の表示を要求する。設定が既にドッキング済みならクローズせず、その画面を再描画させる。
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
        "最新版なら非表示のはず"
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
    assert_eq!(opened(), 1, "設定をドッキングするはず");
    assert!(
        cx.update(|_, cx| updater::take_reveal(cx)),
        "アップデート画面の表示を要求するはず"
    );
    // 設定（パネル 7）がドッキング中に押下すると、クローズも再生成もせず、再描画のみ行う。
    bar.update(cx, |d, cx| d.track(Page::Settings, 7, cx));
    cx.update(|_, cx| ui::dock(7, cx));
    redraw_bar(cx);
    click(cx, "dashboard-update");
    assert_eq!(opened(), 1, "再生成しないはず");
    assert!(!commands(cx, 7).contains(&PanelCommand::Close));
    assert!(cx.update(|_, cx| cx.global::<PanelControl>().panels[&7].redraw));
    assert!(cx.update(|_, cx| updater::take_reveal(cx)));
    // リリースが消えれば（インストール後等）、ボタンも消える。
    cx.update(|_, cx| cx.default_global::<updater::Updater>().found.clear());
    redraw_bar(cx);
    assert!(cx.debug_bounds("dashboard-update").is_none());
}

/// メディア再生中の場合のみ、バーに再生中画面のボタンが表示される。
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

/// 表示中に空領域を押下すると非表示にする（表示開始時点までのカウントでは非表示にしない）。
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

/// 非表示中の押下分は、再表示と同一フレームの状態書き戻しで到着しても非表示にしない（開始音のみ再生されて表示されない状態を防ぐ）。
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
    // 再表示と同一フレームに空領域押下カウントも増加した。
    set(cx, 5, false);
    assert!(!commands(cx, 1).contains(&PanelCommand::Hide(true)));
    set(cx, 6, false);
    assert!(commands(cx, 1).contains(&PanelCommand::Hide(true)));
}

/// ダッシュボードを再表示すると、ドッキング中の画面（配信の場合はそのウィンドウ）の前面化を要求する。
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

/// スライドインアニメーション中の幅計算（クイック設定の自然幅）がアニメーション完了後の実際の幅と一致する。不一致だと完了時にジャンプする。
/// 通知がある場合は時刻の横に件数を表示する（0 件なら非表示）。
#[gpui::test]
fn quick_settings_width_matches_the_laid_out_button(cx: &mut TestAppContext) {
    for count in [0, 12] {
        let (_bar, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().notifications =
                (0..count).map(|id| Arc::new(note(id))).collect();
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
        let shown = cx.debug_bounds("dashboard-notification-count").is_some();
        assert_eq!(shown, count > 0, "件数表示の有無");
        let digits = (count > 0).then(|| {
            cx.update(|window, _| {
                f32::from(design::font::width(
                    count.to_string(),
                    font_size::MD,
                    FontWeight::BOLD,
                    window,
                ))
            })
        });
        let expected = quick_settings_width(f32::from(time), digits);
        assert!(
            (actual - expected).abs() <= 1.0,
            "通知 {count}: {actual} / {expected}"
        );
    }
}

/// 配信が蓄積しても、ボタンは最大 10 個に収まるため、バーは最大幅を超えず横スクロールも不要になる。
#[gpui::test]
fn bar_stays_within_the_max_width_however_many_streams_open(cx: &mut TestAppContext) {
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
    assert!(width <= MAX_BAR_PX + 1.0, "上限を超えない: {width}");
    assert_eq!(bar.read_with(cx, |d, _| d.scroll.max_offset().x), px(0.0));
}

/// バーはウィンドウ全幅には広がらず、内容幅に収まって中央配置される。
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
        "ウィンドウ全幅に広がらないはず: {content:?} / {width:?}"
    );
    let (left, right) = (first.left(), width - last.right());
    assert!(
        (left - right).abs() <= px(2.0),
        "中央配置のはず: {left:?} / {right:?}"
    );
}

/// 開いた配信はバーに蓄積され、押下でドッキングする。ドッキング中の配信はクローズせず非表示にして取り外し、再押下で非表示化する。
/// クローズ済み配信のボタンは除去される。
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
    // 配信は区切り線の右にクリック可能なボタンとして並ぶ。
    assert!(cx.debug_bounds("dashboard-stream-4").is_some());
    click(cx, "dashboard-stream-5");
    assert_eq!(
        commands(cx, 4)[1..],
        [PanelCommand::Hide(true), PanelCommand::Parent(None)]
    );
    // 非表示のままドッキングしてから表示する。
    let five = commands(cx, 5);
    assert!(docked_to_bar(&five[0]));
    assert_eq!(five[1], PanelCommand::FitHeight(SIZE_M.y));
    assert_eq!(
        five[3],
        PanelCommand::FocusSource,
        "配信対象のウィンドウを前面化するはず"
    );
    assert!(
        !commands(cx, 4).contains(&PanelCommand::Close),
        "非表示のみでクローズしないはず"
    );
    // 再押下で非表示化する。
    click(cx, "dashboard-stream-5");
    assert!(commands(cx, 5).contains(&PanelCommand::Hide(true)));
    // クローズ済み配信のボタンは除去される。
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

/// 配信ボタンは最大 10 個（9 個＋一覧ボタン）。10 個までは全部並び、超えた分は一覧ポップオーバーから選べる。
#[test]
fn at_most_ten_stream_buttons_are_shown() {
    let shown: Vec<_> = [0, 1, 9, 10, 11, 30]
        .into_iter()
        .map(|n| shown_streams(n) + usize::from(n > MAX_STREAM_BUTTONS))
        .collect();
    assert_eq!(shown, [0, 1, 9, 10, 10, 10]);
}

/// テスト用に、配信ボタンの ID を `'static` にして返す。
fn stream_id(id: u64) -> &'static str {
    Box::leak(format!("dashboard-stream-{id}").into_boxed_str())
}

#[gpui::test]
fn extra_streams_move_into_the_overflow_popover(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    let add = |cx: &mut VisualTestContext, ids: std::ops::Range<u64>| {
        bar.update(cx, |d, cx| {
            for id in ids {
                d.add_stream(
                    id as _,
                    SourceKind::Window,
                    format!("W{id}").into(),
                    None,
                    false,
                    cx,
                );
            }
        });
        cx.run_until_parked();
    };
    // ちょうど 10 個なら一覧ボタンは出さず、すべて並ぶ。
    add(cx, 10..20);
    assert!((10..20).all(|id| cx.debug_bounds(stream_id(id)).is_some()));
    assert!(cx.debug_bounds("dashboard-stream-overflow").is_none());
    // 11 個目から、9 個＋一覧ボタンになる。
    add(cx, 20..22);
    assert!((10..19).all(|id| cx.debug_bounds(stream_id(id)).is_some()));
    assert!((19..22).all(|id| cx.debug_bounds(stream_id(id)).is_none()));
    // 一覧ボタンはポップオーバーを開き、あふれた配信（19・20・21）を選ぶとドッキングして閉じる。
    click(cx, "dashboard-stream-overflow");
    let spec = cx.update(|_, cx| cx.default_global::<PanelControl>().spawns.pop().unwrap());
    assert!(spec.parent().is_some_and(|p| p.panel == 1 && p.dismiss));
    let streams = bar.read_with(cx, |d, _| {
        d.overflow()
            .iter()
            .map(|s| (s.panel, s.name.clone(), s.kind))
            .collect()
    });
    let (_menu, mcx) = cx.add_window_view(|_, _| StreamMenu {
        panel: 9,
        dashboard: bar.downgrade(),
        streams,
    });
    mcx.run_until_parked();
    assert!(mcx.debug_bounds(stream_id(18)).is_none());
    for id in 19..22 {
        assert!(mcx.debug_bounds(stream_id(id)).is_some());
    }
    click(mcx, "dashboard-stream-20");
    assert!(commands(mcx, 9).contains(&PanelCommand::Close));
    assert!(commands(mcx, 20).iter().any(docked_to_bar));
}

/// 手動で取り外して独立させた配信のボタンは残り、クローズ時にのみ除去される。押下でダッシュボードに再ドッキングする。
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

/// 常駐配信は、独立させてもバーに残り、格納するとクローズせず非表示になる。
#[gpui::test]
fn resident_streams_stay_on_the_bar_and_minimize(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    bar.update(cx, |d, cx| {
        d.add_stream(4, SourceKind::Screen, "Display 1".into(), None, true, cx)
    });
    // ドッキング後に手動で取り外した（独立させた）。
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
        "独立させてもボタンは残るはず"
    );
    bar.update(cx, |d, cx| d.minimize(4, cx));
    let c = commands(cx, 4);
    assert!(c.contains(&PanelCommand::Hide(true)));
    assert!(!c.contains(&PanelCommand::Close), "クローズしないはず");
    assert!(bar.read_with(cx, |d, _| d.streams[0].stashed));
}

/// トーストから通知センターの表示を要求されると、非表示のダッシュボードを再表示し、表示後にクイック設定（通知一覧）をドッキングする
/// （非表示中に開くとドッキングされず浮いてしまう）。既にドッキング済みならクローズしない。
#[gpui::test]
fn the_toast_opens_the_notification_center(cx: &mut TestAppContext) {
    let (bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
        Dashboard::new(1, summon, summon, cx)
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let p = cx
            .default_global::<PanelControl>()
            .panels
            .entry(1)
            .or_default();
        p.status.hidden = true;
    });
    cx.update(|_, cx| ui::notification::show_center(Some(5), cx));
    cx.run_until_parked();
    assert!(
        take_commands(cx, 1).contains(&PanelCommand::Hide(false)),
        "ダッシュボードを再表示するはず"
    );
    assert!(
        cx.update(|_, cx| cx.global::<PanelControl>().spawns.is_empty()),
        "表示完了前には開かないはず"
    );
    cx.update(|_, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(1)
            .or_default()
            .status
            .hidden = false;
    });
    open_spawned(cx, 2);
    assert_eq!(
        cx.update(|_, cx| bar.read(cx).current(cx)),
        Some(Page::QuickSettings)
    );
    cx.update(|_, cx| ui::notification::show_center(Some(6), cx));
    cx.run_until_parked();
    assert!(
        !commands(cx, 2).contains(&PanelCommand::Close),
        "クローズしないはず"
    );
    assert_eq!(
        cx.update(|_, cx| bar.read(cx).current(cx)),
        Some(Page::QuickSettings)
    );
}

/// 件数は未読通知数。全件既読なら非表示にする。
#[gpui::test]
fn the_count_is_of_unread_notifications(cx: &mut TestAppContext) {
    let (_bar, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().notifications =
            (0..3).map(|id| Arc::new(note(id))).collect();
        Dashboard::new(1, summon, summon, cx)
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("dashboard-notification-count").is_some());
    cx.update(|_, cx| ui::notification::mark_read(0..3, cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("dashboard-notification-count").is_none());
}
