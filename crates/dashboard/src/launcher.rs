//! ランチャー: ニャプリ・起動中のアプリ・すべてのアプリを、見出しとタイルの行の仮想化したリストに並べる。
//! 押すとニャプリをはめ、アプリはウィンドウを映す（無ければ起動して待つ。`mirror.rs`）。

use super::*;

/// アプリ一覧の列の数と、起動中のアプリの印（点）の大きさ（px）。
const APP_COLUMNS: u16 = 6;
const RUNNING_DOT_PX: f32 = 6.0;

/// 読んだアプリ一覧。次に開いたときに、取り直すまでの間それを出す（アイコンを読むので時間が掛かる）。
pub(crate) struct AppCatalog(pub(crate) AppList);

/// アプリ一覧（名前の順）と、そのアイコンの画像。
type AppList = Vec<(InstalledApp, Option<Arc<RenderImage>>)>;

impl gpui::Global for AppCatalog {}

/// アプリ一覧を OS から取り直し、アイコンを画像にする（どちらも別スレッド。取得はアイコンを読むので数秒かかる）。
pub(crate) fn fetch_apps(cx: &App) -> Task<Result<AppList, String>> {
    let pending = InstalledApps::refresh();
    let executor = cx.background_executor().clone();
    cx.background_executor().spawn(async move {
        let pending = pending.map_err(|e| e.to_string())?;
        let apps = loop {
            if let Some(result) = pending.poll() {
                break result.map_err(|e| e.to_string())?;
            }
            executor.timer(Duration::from_millis(100)).await;
        };
        Ok(apps
            .into_iter()
            .map(|a| {
                let image = a.icon.as_ref().and_then(render_image);
                (a, image)
            })
            .collect())
    })
}

/// ランチャーの中身: 行（リストの 1 行）と、行が並べるタイル。
#[derive(Default)]
pub(crate) struct Launcher {
    rows: Vec<LauncherRow>,
    tiles: Vec<Tile>,
    /// 起動中のアプリ（[`Tile::Running`] が指す）。
    running: Vec<RunningApp>,
}

impl Launcher {
    /// タイルを足し、[`APP_COLUMNS`] 個ずつの行にする。
    fn push_tiles(&mut self, tiles: Vec<Tile>) {
        let start = self.tiles.len();
        self.tiles.extend(tiles);
        let end = self.tiles.len();
        for row in (start..end).step_by(APP_COLUMNS as usize) {
            self.rows.push(LauncherRow::Tiles(
                row..(row + APP_COLUMNS as usize).min(end),
            ));
        }
    }
}

/// ランチャーのリストの 1 行。
#[derive(Debug, Clone, PartialEq)]
enum LauncherRow {
    NyappsHeader,
    RunningHeader,
    AllAppsHeader,
    Note(SharedString),
    /// 読み込み中（スピナー付きの注記）。
    Loading(SharedString),
    Separator,
    /// [`Launcher::tiles`] のこの範囲を 1 行に並べる。
    Tiles(Range<usize>),
}

/// ランチャーのタイル。`n` は検索で絞った中での順番（`debug_selector` の番号）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tile {
    /// [`Screen::nyapps`] の添字。
    Nyapp(usize),
    /// [`Launcher::running`] の添字。
    Running { at: usize, n: usize },
    /// [`Screen::apps`] の添字と、起動中か。
    App { at: usize, n: usize, running: bool },
}

impl Screen {
    /// 入っているニャプリを読み直す（パッケージを読むので別スレッド）。
    pub(crate) fn scan_nyapps(&mut self, cx: &mut Context<Self>) {
        self.nyapps_pending = true;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async { nyapp::installed::scan() })
                .await;
            let _ = this.update(cx, |s, cx| {
                (s.nyapps, s.nyapps_pending) = (found, false);
                ui::notify(s.panel, cx);
            });
        })
        .detach();
    }

    /// ニャプリ `app` を消す（開いていれば閉じる）。消したら一覧を読み直す。
    fn uninstall_nyapp(&mut self, app: nyapp::Installed, cx: &mut Context<Self>) {
        if let Some(panel) = nyapp::panel_of(&app.manifest.id, cx) {
            close(panel, cx);
        }
        let id = app.manifest.id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { nyapp::installed::uninstall(&id) })
                .await;
            let removed = result.is_ok();
            if let Err(e) = result {
                logger::warn!("cannot uninstall nyapp", app = app.manifest.id, error = e);
            }
            let _ = this.update(cx, |s, cx| {
                if removed {
                    nyapp::activity::clear(&app.manifest.id, cx);
                }
                s.scan_nyapps(cx)
            });
        })
        .detach();
    }

    /// 更新情報を持つニャプリの最新版を確かめ、新しければ入れ替える（ネットワークを使うので別スレッド）。
    fn update_nyapps(&mut self, cx: &mut Context<Self>) {
        self.update_note = Some(i18n::t(cx, "dashboard.nyapps_checking"));
        let apps = self.nyapps.clone();
        cx.spawn(async move |this, cx| {
            let results: Vec<_> = cx
                .background_executor()
                .spawn(async move { apps.iter().map(nyapp::update::update).collect() })
                .await;
            let _ = this.update(cx, |s, cx| {
                let updated = results.iter().filter(|r| matches!(r, Ok(Some(_)))).count();
                let error = results.iter().find_map(|r| r.as_ref().err());
                s.update_note = Some(match (error, updated) {
                    (Some(e), _) => i18n::tf(cx, "dashboard.nyapps_update_failed", &[("error", e)]),
                    (None, 0) => i18n::t(cx, "dashboard.nyapps_up_to_date"),
                    (None, n) => i18n::tf(cx, "dashboard.nyapps_updated", &[("count", &n)]),
                });
                s.scan_nyapps(cx);
            });
        })
        .detach();
    }

    /// ランチャーのニャプリの見出し: 更新の確認と、消す状態の切り替え。
    fn nyapps_header(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (remove_glyph, remove_key, remove_variant) = if self.removing {
            (&icons::CHECK, "dashboard.nyapps_done", Variant::Primary)
        } else {
            (
                &icons::PENCIL_SIMPLE,
                "dashboard.nyapps_edit",
                Variant::OnBackground,
            )
        };
        section(theme, i18n::t(cx, "dashboard.nyapps"))
            .child(
                div()
                    .flex()
                    .gap(px(spacing::XS))
                    .child(
                        components::icon_button(
                            theme,
                            "dashboard-nyapps-update",
                            &icons::ARROW_CIRCLE_UP,
                            i18n::t(cx, "dashboard.nyapps_update"),
                            Variant::OnBackground,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update_nyapps(cx);
                            ui::notify(this.panel, cx);
                        })),
                    )
                    .child(
                        components::icon_button(
                            theme,
                            "dashboard-nyapps-remove",
                            remove_glyph,
                            i18n::t(cx, remove_key),
                            remove_variant,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.removing = !this.removing;
                            ui::notify(this.panel, cx);
                        })),
                    ),
            )
            .into_any_element()
    }

    /// ニャプリ `app` のタイル。押すと開く（消す状態なら消す。アイコンはごみ箱になる）。
    fn nyapp_tile(
        &self,
        app: &nyapp::Installed,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let lang = i18n::Lang::current(cx);
        let name = app.manifest.name_in(lang.code()).to_owned();
        let running = nyapp::panel_of(&app.manifest.id, cx).is_some();
        let icon = if self.removing {
            design::icon(&icons::TRASH)
                .size(px(TILE_ICON_PX))
                .color(rgba(theme.danger_solid))
                .into_any_element()
        } else {
            app.icon(lang.code(), f32::from(tile_icon_size()), theme)
        };
        let tooltip = self
            .removing
            .then(|| i18n::tf(cx, "dashboard.nyapps_uninstall", &[("name", &name)]));
        let app = app.clone();
        let tile = app_tile(
            theme,
            format!("dashboard-nyapp-{}", app.manifest.id),
            icon,
            &name,
            running,
            window,
        );
        match tooltip {
            Some(t) => tile.tooltip(t),
            None => tile,
        }
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.removing {
                this.uninstall_nyapp(app.clone(), cx);
            } else {
                let app = app.clone();
                let _ = this.dashboard.update(cx, |d, cx| d.open_nyapp(app, cx));
            }
            ui::notify(this.panel, cx);
        }))
        .into_any_element()
    }

    /// アプリ `app` のウィンドウ（取得元の一覧から。[`is_window_of`]）。
    fn window_for(&self, app: &InstalledApp) -> Option<&(Source, Option<Arc<RenderImage>>)> {
        self.sources.iter().find(|(s, _)| app.owns(s))
    }

    /// ウィンドウ（画面）を映す。もう映していれば新しく開かず、ダッシュボードに隠している配信ならそれを出す。
    /// 映したものは、この一覧がはまっていればダッシュボードにはまり（一覧と入れ替わる）、独立させていれば独立して開く。
    fn open_window(&self, source: Source, icon: Option<Arc<RenderImage>>, cx: &mut App) {
        if let Some((id, _)) = mirror_of(&source, cx) {
            let _ = self.dashboard.update(cx, |d, cx| d.show_stream(id, cx));
            return;
        }
        let docked = ui::docked(cx) == Some(self.panel);
        cx.default_global::<PanelControl>().spawns.push(mirror_spec(
            source,
            icon,
            Some(self.dashboard.clone()),
            self.panel,
            false,
            docked,
        ));
    }

    /// アプリを開く: 起動していればそのウィンドウを映す。していなければ起動して、すぐそのアプリの画面を開いて待つ
    /// （[`launch_spec`]。ウィンドウが出たら、そこで映し始める）。
    fn launch_app(&mut self, app: InstalledApp, cx: &mut App) {
        if let Some((source, icon)) = self.window_for(&app).cloned() {
            self.open_window(source, icon, cx);
            return;
        }
        match kernel::desktop::launch(&app) {
            Ok(()) => {
                self.error = None;
                let icon = self
                    .apps
                    .iter()
                    .find(|(a, _)| a.id == app.id)
                    .and_then(|(_, i)| i.clone());
                let docked = ui::docked(cx) == Some(self.panel);
                cx.default_global::<PanelControl>().spawns.push(launch_spec(
                    app,
                    icon,
                    Some(self.dashboard.clone()),
                    self.panel,
                    docked,
                ));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// ランチャー: 上に検索欄、その下をスクロールする 1 本のリストにして、ニャプリ・区切り線・起動中のアプリ・区切り線・
    /// 全部のアプリ（名前の順）を並べる。検索すると、どれも名前に打った文字を含むものだけになる。
    /// リストは見えている行だけを組み立てる（アプリは数百個あり、全部のタイルを毎回組むと 1 フレームに収まらない）。
    pub(crate) fn apps_page(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        self.poll_sources();
        let launcher = self.layout_launcher(cx);
        if launcher.rows != self.launcher.rows {
            // 行が変わったときだけ測り直す（同じなら、見えている行を描き直すだけ）。
            self.list
                .splice(0..self.launcher.rows.len(), launcher.rows.len());
        }
        self.launcher = launcher;
        components::panel(theme).child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(spacing::SM))
                .child(self.search_field(theme, cx))
                .when_some(self.error.clone(), |d, error| {
                    d.child(
                        div()
                            .px(px(spacing::SM))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::tf(cx, "dashboard.failed", &[("error", &error)])),
                    )
                })
                .child(
                    list(
                        self.list.clone(),
                        cx.processor(|this, ix: usize, window, cx| {
                            this.launcher_row(ix, window, cx)
                        }),
                    )
                    .flex_1()
                    .min_h_0(),
                ),
        )
    }

    /// ランチャーの行とタイルを、今の一覧と検索から決める（要素は作らない。描くのは見えている行だけ）。
    fn layout_launcher(&self, cx: &App) -> Launcher {
        let query = self.query.trim().to_lowercase();
        let matches = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
        let lang = i18n::Lang::current(cx).code();
        let mut l = Launcher::default();
        let note = |key: &str| LauncherRow::Note(i18n::t(cx, key));

        // 通常のニャプリを止めているときは、見出しごと出さない。
        if nyapp::NORMAL_NYAPPS_ENABLED {
            l.rows.push(LauncherRow::NyappsHeader);
            if let Some(n) = &self.update_note {
                l.rows.push(LauncherRow::Note(n.clone()));
            }
            let nyapps: Vec<_> = (0..self.nyapps.len())
                .filter(|&i| matches(self.nyapps[i].manifest.name_in(lang)))
                .map(Tile::Nyapp)
                .collect();
            if !nyapps.is_empty() {
                l.push_tiles(nyapps);
            } else if self.nyapps_pending {
                l.rows
                    .push(LauncherRow::Loading(i18n::t(cx, "dashboard.loading")));
            } else if self.nyapps.is_empty() {
                let dir = nyapp::installed::dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                l.rows.push(LauncherRow::Note(i18n::tf(
                    cx,
                    "dashboard.nyapps_none",
                    &[("dir", &dir)],
                )));
            } else {
                l.rows.push(note("dashboard.none"));
            }
            l.rows.push(LauncherRow::Separator);
        }

        l.rows.push(LauncherRow::RunningHeader);
        l.running = self.running_apps(cx);
        let running: Vec<_> = l
            .running
            .iter()
            .enumerate()
            .filter(|(_, r)| matches(&r.name))
            .enumerate()
            .map(|(n, (at, _))| Tile::Running { at, n })
            .collect();
        if running.is_empty() {
            l.rows.push(if self.pending.is_some() {
                LauncherRow::Loading(i18n::t(cx, "dashboard.loading"))
            } else {
                note("dashboard.none")
            });
        } else {
            l.push_tiles(running);
        }

        l.rows
            .extend([LauncherRow::Separator, LauncherRow::AllAppsHeader]);
        // 起動中か（点を付ける）は、ウィンドウの実行ファイル名・名前を一度だけ小文字にして突き合わせる。
        let window_exes: HashSet<String> = self
            .sources
            .iter()
            .filter(|(s, _)| s.kind == SourceKind::Window)
            .filter_map(|(s, _)| s.app.as_deref().map(str::to_ascii_lowercase))
            .collect();
        let window_names: Vec<String> = self
            .sources
            .iter()
            .filter(|(s, _)| s.kind == SourceKind::Window)
            .map(|(s, _)| s.name.to_lowercase())
            .collect();
        let apps: Vec<_> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, (a, _))| matches(&a.name))
            .enumerate()
            .map(|(n, (at, (a, _)))| {
                let running = match &a.exe {
                    Some(exe) => window_exes.contains(&exe.to_ascii_lowercase()),
                    None => {
                        let name = a.name.to_lowercase();
                        !name.is_empty() && window_names.iter().any(|w| w.contains(&name))
                    }
                };
                Tile::App { at, n, running }
            })
            .collect();
        if apps.is_empty() {
            l.rows.push(if self.apps_pending {
                LauncherRow::Loading(i18n::t(cx, "dashboard.loading"))
            } else {
                note("dashboard.none")
            });
        } else {
            l.push_tiles(apps);
        }
        l
    }

    /// 起動中のウィンドウを個別に出す。常駐の画面はバーにあるので出さない。
    fn running_apps(&self, cx: &App) -> Vec<RunningApp> {
        // 実行ファイル名（小文字・.exe 抜き）→ アプリ。ウィンドウごとに全アプリを探さないよう、先に一度だけ作る。
        let by_exe: std::collections::HashMap<String, &(InstalledApp, Option<Arc<RenderImage>>)> =
            self.apps
                .iter()
                .filter_map(|a| Some((app_key(a.0.exe.as_deref()?), a)))
                .collect();
        let mut running: Vec<RunningApp> = Vec::new();
        for (source, image) in &self.sources {
            if mirror_of(source, cx).is_some_and(|(_, resident)| resident) {
                continue;
            }
            let key = source
                .app
                .as_deref()
                .map_or_else(|| source.name.clone(), app_key);
            let app = by_exe.get(&key).copied();
            let mut name = match app {
                Some((a, _)) => format!("{} · {}", a.name, source.name),
                None => source.name.clone(),
            };
            let number = running
                .iter()
                .filter(|r| r.source.name == source.name && r.source.app == source.app)
                .count()
                + 1;
            if number > 1 {
                name.push_str(&format!(" ({number})"));
            }
            running.push(RunningApp {
                icon: image.clone().or_else(|| app.and_then(|(_, i)| i.clone())),
                name,
                source: source.clone(),
                window_icon: image.clone(),
            });
        }
        running
    }

    /// ランチャーの `ix` 行目を組み立てる（リストが見えている行だけ呼ぶ）。リストの行は中身の幅に縮むので、
    /// 幅いっぱいに広げる（見出しのボタンを右端に、区切り線・タイルの格子を端から端まで）。
    fn launcher_row(
        &mut self,
        ix: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::current(cx);
        let Some(row) = self.launcher.rows.get(ix).cloned() else {
            return div().into_any_element();
        };
        let row = match row {
            LauncherRow::NyappsHeader => self.nyapps_header(&theme, cx),
            LauncherRow::RunningHeader => section(&theme, i18n::t(cx, "dashboard.running"))
                .child(
                    components::icon_button(
                        &theme,
                        "dashboard-refresh",
                        &icons::ARROW_CLOCKWISE,
                        i18n::t(cx, "dashboard.refresh"),
                        Variant::OnBackground,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh();
                        ui::notify(this.panel, cx);
                    })),
                )
                .into_any_element(),
            LauncherRow::AllAppsHeader => {
                section(&theme, i18n::t(cx, "dashboard.all_apps")).into_any_element()
            }
            LauncherRow::Note(text) => div()
                .px(px(spacing::SM))
                .pb(px(spacing::SM))
                .text_color(rgba(theme.neutral_text_low))
                .child(text)
                .into_any_element(),
            LauncherRow::Loading(text) => div()
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .px(px(spacing::SM))
                .pb(px(spacing::SM))
                .text_color(rgba(theme.neutral_text_low))
                .child(components::spinner(&theme, 16.0))
                .child(text)
                .into_any_element(),
            LauncherRow::Separator => div()
                .py(px(spacing::SM))
                .child(
                    div()
                        .h(px(1.0))
                        .mx(px(spacing::SM))
                        .bg(rgba(theme.neutral_border_subtle)),
                )
                .into_any_element(),
            LauncherRow::Tiles(range) => {
                let tiles: Vec<_> = range
                    .map(|i| self.launcher_tile(i, &theme, window, cx))
                    .collect();
                app_grid()
                    .pb(px(spacing::XS))
                    .children(tiles)
                    .into_any_element()
            }
        };
        div().w_full().child(row).into_any_element()
    }

    fn launcher_tile(
        &self,
        i: usize,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match self.launcher.tiles[i] {
            Tile::Nyapp(at) => self.nyapp_tile(&self.nyapps[at], theme, window, cx),
            Tile::Running { at, n } => {
                let r = &self.launcher.running[at];
                let (source, image) = (r.source.clone(), r.window_icon.clone());
                let glyph = match source.kind {
                    SourceKind::Screen => &icons::MONITOR,
                    SourceKind::Window => &icons::APP_WINDOW,
                };
                app_tile(
                    theme,
                    format!("dashboard-running-{n}"),
                    tile_image(r.icon.clone(), glyph),
                    &r.name,
                    true,
                    window,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_window(source.clone(), image.clone(), cx);
                    ui::notify(this.panel, cx);
                }))
                .into_any_element()
            }
            Tile::App { at, n, running } => {
                let (app, icon) = &self.apps[at];
                let app = app.clone();
                app_tile(
                    theme,
                    format!("dashboard-app-{n}"),
                    tile_image(icon.clone(), &icons::APP_WINDOW),
                    &app.name,
                    running,
                    window,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.launch_app(app.clone(), cx);
                    ui::notify(this.panel, cx);
                }))
                .into_any_element()
            }
        }
    }

    /// アプリ一覧の検索欄。押すと文字を打てる（仮想キーボードが出る）。打っている間は右端に消すボタンを出す。
    fn search_field(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.query.is_empty();
        let focus = self.search.clone();
        div()
            .id("dashboard-search")
            .debug_selector(|| "dashboard-search".into())
            .track_focus(&self.search)
            .flex_none()
            .h(px(components::NAV_BAR_HEIGHT - 2.0 * spacing::SM))
            .flex()
            .items_center()
            .gap(px(spacing::SM))
            .pl(px(spacing::LG))
            .pr(px(spacing::XS))
            .rounded(px(design::radius::FULL))
            .bg(rgba(theme.control))
            .on_click(cx.listener(move |this, _, window, cx| {
                ui::keyboard::focus_input(this.panel, &focus, window, cx)
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.control || event.keystroke.modifiers.platform {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "backspace" => {
                        this.query.pop();
                    }
                    "enter" | "tab" | "escape" => return,
                    _ => match &event.keystroke.key_char {
                        Some(text) => this.query.push_str(text),
                        None => return,
                    },
                }
                ui::notify(this.panel, cx);
            }))
            .child(design::icon(&icons::MAGNIFYING_GLASS).size(px(font_size::LG)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(empty, |d| d.text_color(rgba(theme.neutral_text_low)))
                    .child(if empty {
                        i18n::t(cx, "dashboard.search_apps")
                    } else {
                        self.query.clone().into()
                    }),
            )
            .when(!empty, |d| {
                d.child(
                    components::icon_button(
                        theme,
                        "dashboard-search-clear",
                        &icons::X,
                        i18n::t(cx, "dashboard.clear_search"),
                        Variant::Ghost,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.query.clear();
                        ui::notify(this.panel, cx);
                    })),
                )
            })
    }
}

/// アプリ一覧のグリッド（アプリは地を敷かないので、タイルより多く並べる）。
fn app_grid() -> Div {
    div().grid().grid_cols(APP_COLUMNS).gap(px(spacing::XS))
}

/// アプリ一覧の起動中のウィンドウ。
struct RunningApp {
    name: String,
    icon: Option<Arc<RenderImage>>,
    /// 開くウィンドウと、そのアイコン。
    source: Source,
    window_icon: Option<Arc<RenderImage>>,
}

/// アプリ一覧のアプリ: アイコン（`icon`）の下に名前（地を敷かず、ホバー・押下だけ色が付く）。起動中・オーバーレイで
/// 開いているもの（`running`）は、その下に点を付ける。
fn app_tile(
    theme: &Theme,
    id: String,
    icon: gpui::AnyElement,
    name: &str,
    running: bool,
    window: &Window,
) -> components::Button {
    let tile = components::ghost_tile(theme, id)
        .h(tile_height())
        .min_w_0()
        .child(icon)
        .child(tile_label(name.to_owned()))
        // 点が無くても場所は取り、アイコンと名前の高さをそろえる。
        .child(
            div()
                .debug_selector(|| "dashboard-running-dot".into())
                .flex_none()
                .size(px(RUNNING_DOT_PX))
                .rounded(px(design::radius::FULL))
                .when(running, |d| d.bg(rgba(theme.neutral_text))),
        );
    if app_label_clipped(name, window) {
        tile.tooltip(name.to_owned())
    } else {
        tile
    }
}

fn app_label_clipped(name: &str, window: &Window) -> bool {
    let width = f32::from(window.viewport_size().width);
    let available = (width
        - 2.0 * (components::PANEL_MARGIN + spacing::MD)
        - (APP_COLUMNS as f32 - 1.0) * spacing::XS)
        / APP_COLUMNS as f32
        - 2.0 * spacing::MD;
    let width = design::font::width(name.to_owned(), font_size::MD, FontWeight::NORMAL, window);
    f32::from(width) > available
}

#[cfg(test)]
mod tests;
