//! 配信を映すパネル（[`Mirror`]）と、ランチャーから起動したアプリのウィンドウを待つ画面。

use super::*;

/// アプリを起動してから、ウィンドウが出るのを待つ長さ。出なければ待機中の画面に見つからないと出す（起動はしている）。
const LAUNCH_WAIT: Duration = Duration::from_secs(20);
/// 起動したアプリのウィンドウを探し直す間隔。
const LAUNCH_POLL: Duration = Duration::from_millis(500);

/// 配信を映すパネル。画像はカーネルが直接送るので、描くのは始まるまで・終わった後の状態と、起動したアプリのウィンドウを
/// 待っている間（[`launch_spec`]）だけ。映しているウィンドウが閉じたら、パネルも閉じる。
pub(crate) struct Mirror {
    panel: PanelId,
    /// 映しているもの（ウィンドウを待っている間は `None`）。
    source: Option<Source>,
    /// 起動したアプリのウィンドウを待っている。
    waiting: Option<Waiting>,
    /// 映し始めたら配信のボタンを足し、最小化時にしまうダッシュボード。
    dashboard: Option<WeakEntity<Dashboard>>,
    /// 配信の再接続を探している（状態が書き戻されるたびに繰り返さない）。
    ending: bool,
    watch_generation: u64,
}

/// 起動したアプリのウィンドウを待っている間の中身。
pub(crate) struct Waiting {
    app: InstalledApp,
    icon: Option<Arc<RenderImage>>,
    /// 待つのをやめた（[`LAUNCH_WAIT`] の間に出なかった）。
    gave_up: bool,
}

impl Mirror {
    pub(crate) fn new(
        panel: PanelId,
        source: Option<Source>,
        waiting: Option<Waiting>,
        dashboard: Option<WeakEntity<Dashboard>>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global::<PanelControl>(Self::follow_status)
            .detach();
        let mut this = Self {
            panel,
            source,
            waiting,
            dashboard,
            ending: false,
            watch_generation: 0,
        };
        if this.waiting.is_some() {
            this.wait_for_window(cx);
        } else {
            this.watch_successor(cx);
        }
        this
    }

    /// 取得元が入れ替わったら配信をつなぎ直す。OS での最小化は、ダッシュボードのしまう動作とは連動しない。
    fn follow_status(&mut self, cx: &mut Context<Self>) {
        let status = cx
            .global::<PanelControl>()
            .panels
            .get(&self.panel)
            .and_then(|p| p.status.desktop.clone());
        if matches!(status, Some(Status::Starting | Status::Streaming)) {
            self.ending = false;
            return;
        }
        if self.ending {
            return;
        }
        match status {
            Some(Status::Minimized) => {
                self.ending = true;
                self.watch_generation += 1;
                ui::notify(self.panel, cx);
            }
            Some(Status::Closed | Status::Failed(_))
                if self
                    .source
                    .as_ref()
                    .is_some_and(|s| s.kind == SourceKind::Window) =>
            {
                self.ending = true;
                ui::notify(self.panel, cx);
                self.watch_successor(cx);
            }
            _ => {}
        }
    }

    fn restart(&mut self, source: Source, cx: &mut Context<Self>) {
        self.source = Some(source.clone());
        if let Some(mirrors) = cx.try_global::<Mirrors>() {
            let mut mirrors = mirrors.0.clone();
            if let Some((current, _, _)) = mirrors.iter_mut().find(|(_, id, _)| *id == self.panel) {
                *current = source.clone();
            }
            cx.set_global(Mirrors(mirrors));
        }
        commands(self.panel, [stream(source, cx)], cx);
        ui::notify(self.panel, cx);
        self.watch_successor(cx);
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.source.clone() else {
            return;
        };
        self.ending = true;
        self.restart(source, cx);
    }

    /// 起動直後に本画面が別窓へ移るアプリを追う。既存の別窓は候補にしない。
    fn watch_successor(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self
            .source
            .clone()
            .filter(|s| s.kind == SourceKind::Window && s.concrete())
        else {
            return;
        };
        self.watch_generation += 1;
        let generation = self.watch_generation;
        cx.spawn(async move |this, cx| {
            let Ok(pending) = Sources::refresh() else {
                return;
            };
            let known = loop {
                if let Some(result) = pending.poll() {
                    break result;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            };
            let Ok(known) = known else { return };
            let started = Instant::now();
            while started.elapsed() < LAUNCH_WAIT {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let active = this.update(cx, |m, _| {
                    m.watch_generation == generation
                        && m.source.as_ref().is_some_and(|s| s.same_target(&source))
                });
                if !matches!(active, Ok(true)) {
                    return;
                }
                let Ok(pending) = Sources::refresh() else {
                    continue;
                };
                let sources = loop {
                    if let Some(result) = pending.poll() {
                        break result;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                };
                if let Ok(sources) = sources
                    && let Some(next) = source.successor(&known, &sources)
                {
                    let _ = this.update(cx, |m, cx| {
                        if m.watch_generation == generation {
                            m.restart(next, cx);
                        }
                    });
                    return;
                }
            }
        })
        .detach();
    }

    /// 起動したアプリのウィンドウを探し続け、出たら映し始める。[`LAUNCH_WAIT`] の間に出なければ諦める。
    fn wait_for_window(&self, cx: &mut Context<Self>) {
        let Some(app) = self.waiting.as_ref().map(|w| w.app.clone()) else {
            return;
        };
        let started = Instant::now();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(LAUNCH_POLL).await;
                let found = match Sources::refresh() {
                    Ok(pending) => loop {
                        if let Some(result) = pending.poll() {
                            break result
                                .ok()
                                .and_then(|s| s.into_iter().find(|s| app.owns(s)));
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await;
                    },
                    Err(_) => None,
                };
                let gave_up = started.elapsed() > LAUNCH_WAIT;
                let done = this.update(cx, |m, cx| match found {
                    Some(source) => {
                        m.found(source, cx);
                        true
                    }
                    None if gave_up => {
                        if let Some(w) = &mut m.waiting {
                            w.gave_up = true;
                        }
                        ui::notify(m.panel, cx);
                        true
                    }
                    None => false,
                });
                if !matches!(done, Ok(false)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// 待っていたウィンドウ `source` が出た: このパネルで映し始め、ダッシュボードに配信のボタンを足す。
    pub(crate) fn found(&mut self, source: Source, cx: &mut Context<Self>) {
        let icon = self.waiting.take().and_then(|w| w.icon);
        self.source = Some(source.clone());
        start_mirror(self.panel, source, icon, self.dashboard.clone(), false, cx);
        ui::notify(self.panel, cx);
        self.watch_successor(cx);
    }
}

impl Render for Mirror {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let state = match &self.waiting {
            Some(w) if w.gave_up => MirrorState::LaunchFailed,
            Some(_) => MirrorState::Launching,
            None => MirrorState::from_status(
                cx.try_global::<PanelControl>()
                    .and_then(|c| c.panels.get(&self.panel))
                    .and_then(|s| s.status.desktop.clone()),
            ),
        };
        let selector = if self.waiting.is_some() {
            "mirror-waiting"
        } else {
            "mirror-status"
        };
        let action = state.action().map(|action| {
            components::button(&theme, action.id(), action.label(cx), Variant::Default)
                .on_click(cx.listener(|this, _, _, cx| this.retry(cx)))
                .into_any_element()
        });
        mirror_screen(&theme, &state, action, cx).debug_selector(move || selector.into())
    }
}

/// パネル `panel` で `source` を映し始める: 映しているものとして覚え、配信を頼み、`dashboard` があれば配信のボタンを足す。
fn start_mirror(
    panel: PanelId,
    source: Source,
    icon: Option<Arc<RenderImage>>,
    dashboard: Option<WeakEntity<Dashboard>>,
    resident: bool,
    cx: &mut App,
) {
    // 閉じたものは忘れ、これを覚える。
    let closed = |id: &PanelId| {
        cx.try_global::<PanelControl>()
            .and_then(|c| c.panels.get(id))
            .is_some_and(|p| p.status.closed)
    };
    let mut mirrors: Vec<_> = cx
        .try_global::<Mirrors>()
        .map(|m| {
            m.0.iter()
                .filter(|(_, id, _)| !closed(id))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    mirrors.push((source.clone(), panel, resident && dashboard.is_some()));
    cx.set_global(Mirrors(mirrors));
    let (name, kind) = (SharedString::from(source.name.clone()), source.kind);
    commands(panel, [stream(source, cx)], cx);
    if let Some(dashboard) = dashboard {
        let _ = dashboard.update(cx, |d, cx| {
            d.add_stream(panel, kind, name, icon, resident, cx)
        });
    }
}

/// 起動したアプリ `app` の画面: 開いた時点ではアイコンと名前と「起動しています」を出して待ち、ウィンドウが出たらそこで
/// 映し始める（[`Mirror::wait_for_window`]）。`dashboard` があればはめて開く（ランチャーと入れ替わる）。
pub(crate) fn launch_spec(
    app: InstalledApp,
    icon: Option<Arc<RenderImage>>,
    dashboard: Option<WeakEntity<Dashboard>>,
    from: PanelId,
    docked: bool,
) -> PanelSpec {
    let title = app.name.clone();
    let stash = dashboard.clone();
    let spec = PanelSpec::spawn(from, move |panel, _, cx: &mut Context<Mirror>| {
        if docked {
            ui::dock(panel, cx);
        }
        let waiting = Waiting {
            app,
            icon,
            gave_up: false,
        };
        Mirror::new(panel, None, Some(waiting), dashboard, cx)
    });
    // はめるものは、開いた元の手前ではなく、はめる場所に合わせて目の前に出す。
    let spec = if docked {
        spec.in_view(SCREEN_POSE)
    } else {
        spec
    };
    let spec = spec
        .sized(SIZE_M, RES_PX)
        .titled_text(title)
        .dockable()
        .with_os_keyboard();
    match stash {
        Some(dashboard) => spec.source_minimizable(move |id, cx| {
            let _ = dashboard.update(cx, |d, cx| d.minimize(id, cx));
        }),
        None => spec,
    }
}

/// 開いている配信のパネルと、その取得元・常駐か。同じ画面・ウィンドウは 1 枚だけ映す（[`mirror_of`]）。
#[derive(Default)]
struct Mirrors(Vec<(Source, PanelId, bool)>);

impl gpui::Global for Mirrors {}

/// `source` を映している、閉じていないパネルと、それが常駐（起動時に用意した画面）か。
// ponytail: 開くのは次のフレームなので、同じフレームに 2 回押すと 2 枚開く。困れば開く前の分も数える。
pub(crate) fn mirror_of(source: &Source, cx: &App) -> Option<(PanelId, bool)> {
    let panels = &cx.try_global::<PanelControl>()?.panels;
    cx.try_global::<Mirrors>()?
        .0
        .iter()
        .find(|(s, id, _)| {
            s.same_target(source) && !panels.get(id).is_some_and(|p| p.status.closed)
        })
        .map(|&(_, id, resident)| (id, resident))
}

/// `source` を映すパネルの起動仕様。`docked` ならランチャーと入れ替わりにはまり、
/// そうでなければ開いた画面 `from` の手前に重ねて開く。
/// `resident` なら（`dashboard` があるとき）常駐のもの: はめずに隠して開き、バーのボタンで出す。ナビゲーションバーでは
/// 閉じずにしまう。
/// タイトルは取得元の名前で、ナビゲーションバーで閉じると配信も止まる。
pub(crate) fn mirror_spec(
    source: Source,
    icon: Option<Arc<RenderImage>>,
    dashboard: Option<WeakEntity<Dashboard>>,
    from: PanelId,
    resident: bool,
    docked: bool,
) -> PanelSpec {
    let source_title = source.name.clone();
    let source_kind = source.kind;
    let minimize = dashboard.clone().filter(|_| resident);
    let stash = dashboard.clone();
    let build = move |panel: PanelId, _: &mut Window, cx: &mut Context<Mirror>| {
        start_mirror(panel, source.clone(), icon, dashboard.clone(), resident, cx);
        if resident {
            commands(panel, [PanelCommand::Hide(true)], cx);
        } else if docked {
            ui::dock(panel, cx);
        }
        Mirror::new(panel, Some(source), None, dashboard, cx)
    };
    let spec = PanelSpec::spawn(from, build);
    // はめるものは、開いた元の手前ではなく、はめる場所に合わせて目の前に出す。
    let spec = if docked || resident {
        spec.in_view(SCREEN_POSE)
    } else {
        spec
    };
    let spec = match minimize {
        Some(dashboard) => spec.minimizable(move |id, cx| {
            let _ = dashboard.update(cx, |d, cx| d.minimize(id, cx));
        }),
        None => spec,
    };
    // 配信は横幅を保って縦横比を合わせるので、幅をはめる画面とそろえる。
    let spec = spec
        .sized(SIZE_M, RES_PX)
        .titled_text(source_title)
        .dockable()
        // 映しているデスクトップ・ウィンドウへ打てるよう、ナビゲーションバーの左端に OS キーボードのトグルを出す。
        .with_os_keyboard();
    match stash {
        Some(dashboard) if source_kind == SourceKind::Window => {
            spec.source_minimizable(move |id, cx| {
                let _ = dashboard.update(cx, |d, cx| d.minimize(id, cx));
            })
        }
        _ => spec,
    }
}

#[cfg(test)]
mod tests;
