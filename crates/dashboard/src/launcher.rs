//! ランチャー: 起動中のアプリ・すべてのアプリを、見出しとタイルの行の仮想化したリストに並べる。
//! 押すとアプリのウィンドウを映す（無ければ起動して待つ。`mirror.rs`）。

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
    /// [`Launcher::running`] の添字。
    Running { at: usize, n: usize },
    /// [`Screen::apps`] の添字と、起動中か。
    App { at: usize, n: usize, running: bool },
}

impl Screen {
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
    pub(crate) fn launch_app(&mut self, app: InstalledApp, cx: &mut App) {
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

    /// ランチャー: 上に検索欄、その下をスクロールする 1 本のリストにして、起動中のアプリ・区切り線・
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
        let mut l = Launcher::default();
        let note = |key: &str| LauncherRow::Note(i18n::t(cx, key));

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
