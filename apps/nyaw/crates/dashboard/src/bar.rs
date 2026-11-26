//! バー（[`Dashboard`]）: メニュー・クイック設定・再生中・ランチャー・アップデートのボタンと、開いた配信のボタン。
//! 押下で画面をドッキングし、再押下でクローズする。配信は蓄積され、選択すると既にドッキング中のものは非表示にして取り外す。

use super::*;

impl Dashboard {
    pub(crate) fn new(
        panel: PanelId,
        settings: Summon,
        debug: Summon,
        cx: &mut Context<Self>,
    ) -> Self {
        tick_every_second(panel, cx);
        cx.observe_global::<PanelControl>(Self::close_on_outside_press)
            .detach();
        cx.observe_global::<PanelControl>(Self::follow_placement)
            .detach();
        // 件数は未読通知数。
        cx.observe_global::<ui::notification::Read>(|this, cx| ui::notify(this.panel, cx))
            .detach();
        // トーストのカードを押下すると通知センター（クイック設定の一覧）を開く（非表示なら表示する）。
        cx.observe_global::<ui::notification::Center>(Self::open_center)
            .detach();
        cx.observe_global::<PanelControl>(|this, cx| {
            if this.center_waiting && !this.hidden(cx) {
                this.center_waiting = false;
                this.dock_center(cx);
            }
        })
        .detach();
        // 新規リリースの有無は起動時と、常駐して開いたままでも検知できるよう定期的に確認する（あればバーにボタンを表示。
        // インストールはユーザーの明示的な操作のみ）。開発ビルドとテストでは確認しない（設定から手動で実行可能）。
        cx.observe_global::<updater::Updater>(|this, cx| ui::notify(this.panel, cx))
            .detach();
        if !cfg!(debug_assertions) {
            cx.spawn(async move |this, cx| {
                while this.update(cx, |_, cx| updater::check_now(cx)).is_ok() {
                    cx.background_executor().timer(updater::RECHECK).await;
                }
            })
            .detach();
        }
        // アプリ一覧の取得には数秒かかるため、起動時に事前取得する（ランチャー表示時に即座に使用可能にする）。
        let fetch = fetch_apps(cx);
        cx.spawn(async move |_, cx| match fetch.await {
            Ok(apps) => {
                cx.update(|cx| {
                    if !cx.has_global::<AppCatalog>() {
                        cx.set_global(AppCatalog(apps));
                    }
                });
            }
            Err(e) => logger::warn!("cannot list installed apps", error = e),
        })
        .detach();
        // モニター一覧の取得完了後、すべてを配信として事前に開く。非表示中は描画しないため、描画とは別に待機する。
        // テストでは取得しない（OS 一覧の到着タイミングがテストの同期と競合し、配信の開閉状態が非決定的になる）。
        let preload = if cfg!(test) {
            None
        } else {
            Sources::refresh().ok()
        };
        if let Some(pending) = preload {
            cx.spawn(async move |this, cx| {
                let sources = loop {
                    if let Some(result) = pending.poll() {
                        break result;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                };
                match sources {
                    Ok(sources) => {
                        let _ = this.update(cx, |d, cx| d.open_screens(sources, cx));
                    }
                    Err(e) => logger::warn!("cannot list screens to mirror", error = e),
                }
            })
            .detach();
        }
        Self {
            panel,
            settings,
            debug,
            streams: Vec::new(),
            menu: None,
            pages: Vec::new(),
            outside: None,
            placement: None,
            prev_streams: Vec::new(),
            prev_media: false,
            prev_update: false,
            prev_sep: false,
            overflow_menu: None,
            scroll: ScrollHandle::new(),
            enter: Entering::new(cfg!(test)),
            center_asked: cx
                .try_global::<ui::notification::Center>()
                .map_or(0, |c| c.asked),
            center_waiting: false,
        }
    }

    /// 通知センターの開閉要求（トーストのカード押下）を処理する。ダッシュボードを表示して一覧をドッキングする。
    /// ロックダウン中は無視する。
    fn open_center(&mut self, cx: &mut Context<Self>) {
        let asked = cx.global::<ui::notification::Center>().asked;
        if asked == self.center_asked {
            return;
        }
        self.center_asked = asked;
        if cx.global::<PanelControl>().settings.lockdown {
            return;
        }
        if self.hidden(cx) {
            // 表示は次フレームで行われ、非表示中はドッキングできない（画面が浮いたままになる）。表示後にドッキングする。
            show(self.panel, cx);
            self.center_waiting = true;
            return;
        }
        self.dock_center(cx);
    }

    /// 通知センター（クイック設定）をドッキングする（既にドッキング済みの場合は何もしない）。
    fn dock_center(&mut self, cx: &mut Context<Self>) {
        if self.current(cx) != Some(Page::QuickSettings) {
            self.toggle(Page::QuickSettings, cx);
        }
        ui::notify(self.panel, cx);
    }

    fn hidden(&self, cx: &App) -> bool {
        cx.global::<PanelControl>()
            .panels
            .get(&self.panel)
            .is_some_and(|p| p.status.hidden)
    }

    /// 開いているメニューのポップオーバー（外側押下でクローズ済みなら `None`）。
    fn open_menu(&self, cx: &App) -> Option<PanelId> {
        open_popover(self.menu, cx)
    }

    /// 並べきれない配信（[`MAX_STREAM_BUTTONS`] 個を超えた分）。
    fn overflow(&self) -> &[Stream] {
        &self.streams[shown_streams(self.streams.len())..]
    }

    /// 並べきれない配信の一覧ポップオーバーを開閉する（バーの右端に揃える）。
    fn toggle_overflow(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = open_popover(self.overflow_menu, cx) {
            close(id, cx);
            self.overflow_menu = None;
        } else {
            let streams: Vec<_> = self
                .overflow()
                .iter()
                .map(|s| (s.panel, s.name.clone(), s.kind, s.icon.clone()))
                .collect();
            let (bar, dashboard) = (self.panel, cx.weak_entity());
            let rows = streams.len();
            let (size, res) = overflow_size(rows);
            let right_gap = BAR_PX.0 as f32 - f32::from(self.scroll.bounds().right());
            let spec = PanelSpec::popover(bar, move |id, _, cx: &mut Context<StreamMenu>| {
                let _ = dashboard.update(cx, |d, _| d.overflow_menu = Some(id));
                StreamMenu {
                    panel: id,
                    dashboard,
                    streams,
                }
            })
            .sized(size, res)
            .offset(overflow_offset(right_gap, rows))
            .without_resize();
            cx.default_global::<PanelControl>().spawns.push(spec);
        }
        ui::notify(self.panel, cx);
    }

    /// メニューを開閉する。表示中にボタンを押下すると外側押下として扱われ、オーバーレイがクローズする。
    fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.open_menu(cx) {
            close(id, cx);
            self.menu = None;
        } else {
            let (bar, dashboard) = (self.panel, cx.weak_entity());
            let spec = PanelSpec::popover(bar, move |id, _, cx: &mut Context<Menu>| {
                let _ = dashboard.update(cx, |d, _| d.menu = Some(id));
                Menu {
                    panel: id,
                    dashboard,
                }
            })
            .sized(MENU_M, MENU_PX)
            // バーは内容幅で中央配置され、幅は配信等で変動する。現在のバーの左端に合わせて開く。
            .offset(menu_offset(f32::from(self.scroll.bounds().left())))
            .without_resize();
            cx.default_global::<PanelControl>().spawns.push(spec);
        }
        ui::notify(self.panel, cx);
    }

    /// 配信 `panel` を開いた（ドッキング・非表示化は呼び出し側が行う。`resident` なら常駐配信で、非表示で開いた）。バーにボタンを追加する。
    pub(crate) fn add_stream(
        &mut self,
        panel: PanelId,
        kind: SourceKind,
        name: SharedString,
        icon: Option<Arc<RenderImage>>,
        resident: bool,
        cx: &mut Context<Self>,
    ) {
        self.streams.push(Stream {
            panel,
            kind,
            name,
            icon,
            stashed: resident,
        });
        ui::notify(self.panel, cx);
    }

    /// すべてのモニターを常駐配信として非表示で事前に開く（バーのボタンで表示可能。クローズせず格納する）。
    /// 既に配信中のものは開かない。
    fn open_screens(&mut self, mut sources: Vec<Source>, cx: &mut Context<Self>) {
        sources.sort_by(|a, b| a.name.cmp(&b.name));
        for source in sources {
            if source.kind == SourceKind::Screen
                && source.concrete()
                && mirror_of(&source, cx).is_none()
            {
                let spec = mirror_spec(
                    source,
                    None,
                    Some(cx.weak_entity()),
                    self.panel,
                    true,
                    false,
                );
                cx.default_global::<PanelControl>().spawns.push(spec);
            }
        }
    }

    /// ドッキング中の配信を、クローズせず非表示にして取り外す（ボタンで再表示可能）。配信でなければ何もしない
    /// （他の画面は、次にドッキングする画面と入れ替わりにクローズされる）。
    fn stash_docked(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = ui::docked(cx) {
            self.stash(id, cx);
        }
    }

    /// 常駐配信 `id` を格納する（ナビゲーションバーの格納ボタン）。ドッキング中でも独立状態でも、非表示にしてバーに残す。
    pub(crate) fn minimize(&mut self, id: PanelId, cx: &mut Context<Self>) {
        self.stash(id, cx);
        ui::notify(self.panel, cx);
    }

    /// 配信 `id` を、クローズせず非表示にして取り外す。配信でなければ何もしない。
    fn stash(&mut self, id: PanelId, cx: &mut Context<Self>) {
        let Some(stream) = self.streams.iter_mut().find(|s| s.panel == id) else {
            return;
        };
        stream.stashed = true;
        // 非表示の配信には入力できないため、その配信向けのキーボードも解除する。
        if cx
            .try_global::<ui::keyboard::InputRouting>()
            .and_then(|r| r.active)
            .is_some_and(|s| s.target == id)
        {
            ui::keyboard::dismiss_keyboard(cx);
        }
        // 先に非表示にしてから取り外す（表示したまま取り外すと、離脱アニメーションが見えてしまう）。
        commands(id, [PanelCommand::Hide(true)], cx);
        ui::undock(id, cx);
    }

    /// 配信 `id` をドッキングする（ドッキング中の配信は非表示化し、他の画面はクローズする）。既にドッキング済みなら非表示化する。
    pub fn toggle_stream(&mut self, id: PanelId, cx: &mut Context<Self>) {
        let previous = ui::docked(cx);
        let docked = previous == Some(id);
        // 配信間の切り替え時は、表示中の OS キーボードをクローズせず次の配信に引き渡す（非表示化する配信と一緒に解除しない）。
        if !docked
            && let Some(previous) = previous
            && [previous, id]
                .iter()
                .all(|p| self.streams.iter().any(|s| s.panel == *p))
        {
            ui::keyboard::hand_over_os_keyboard(previous, id, cx);
        }
        self.stash_docked(cx);
        if !docked && let Some(stream) = self.streams.iter_mut().find(|s| s.panel == id) {
            stream.stashed = false;
            // 非表示のままドッキングしてから表示する。先に表示すると、非表示時の位置（その後ダッシュボードを移動していれば遠方）から
            // ドッキング位置へスライド・回転して入ってくる。
            ui::dock(id, cx);
            // 配信対象のウィンドウを OS 上で前面化する（そのまま操作できるようにする）。
            commands(
                id,
                [
                    PanelCommand::FitHeight(SIZE_M.y),
                    PanelCommand::Hide(false),
                    PanelCommand::FocusSource,
                ],
                cx,
            );
        }
        ui::notify(self.panel, cx);
    }

    /// 配信 `id` がダッシュボードの配信で、未ドッキングならドッキングする（独立状態のものは移動しない）。
    pub(crate) fn show_stream(&mut self, id: PanelId, cx: &mut Context<Self>) {
        if ui::docked(cx) != Some(id) && self.streams.iter().any(|s| s.panel == id) {
            self.toggle_stream(id, cx);
        }
    }

    /// クローズ済み配信のボタンを除去する。独立状態のものは残す（ボタンで復帰可能）。
    fn prune_streams(&mut self, cx: &App) {
        let ctl = cx.global::<PanelControl>();
        self.streams
            .retain(|s| !ctl.panels.get(&s.panel).is_some_and(|p| p.status.closed));
    }

    /// 現在ドッキング中の画面。
    pub fn current(&self, cx: &App) -> Option<Page> {
        let docked = ui::docked(cx)?;
        self.pages
            .iter()
            .find(|&&(_, id)| id == docked)
            .map(|&(page, _)| page)
    }

    /// `page` の未クローズ画面（ドッキング中でも、取り外して独立させた状態でも）。
    fn page_panel(&self, page: Page, cx: &App) -> Option<PanelId> {
        let closed = |id: &PanelId| {
            cx.try_global::<PanelControl>()
                .and_then(|c| c.panels.get(id))
                .is_some_and(|p| p.status.closed)
        };
        self.pages
            .iter()
            .find(|(p, id)| *p == page && !closed(id))
            .map(|&(_, id)| id)
    }

    /// `page` の画面をドッキングする（ドッキング中の画面はクローズする）。既にドッキング済みならクローズする。
    /// 取り外して独立させた画面が存在すれば、新規に開かずそれを再ドッキングする。
    pub fn toggle(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.current(cx) == Some(page) {
            if let Some(id) = ui::docked(cx) {
                close(id, cx);
                self.pages.retain(|&(_, p)| p != id);
            }
        } else if let Some(id) = self.page_panel(page, cx) {
            self.stash_docked(cx);
            ui::dock(id, cx);
        } else {
            // ドッキング中の配信はクローズせず非表示化する（他の画面は、次の画面と入れ替わりにクローズする）。
            self.stash_docked(cx);
            let dashboard = cx.weak_entity();
            let opened = Box::new(move |id, cx: &mut App| {
                ui::dock(id, cx);
                let _ = dashboard.update(cx, |d, cx| d.track(page, id, cx));
            });
            let spec = match page {
                Page::QuickSettings | Page::NowPlaying | Page::Launcher => {
                    screen_spec(self.panel, page, cx.weak_entity(), opened)
                }
                Page::Debug => (self.debug)(self.panel, opened),
                Page::Settings => (self.settings)(self.panel, opened),
            };
            // ドッキングする画面は、起動元（バー）の手前ではなく、ドッキング位置に合わせて正面に配置する。
            cx.default_global::<PanelControl>()
                .spawns
                .push(spec.fit_height(SIZE_M.y).in_view(SCREEN_POSE).dockable());
        }
        ui::notify(self.panel, cx);
    }

    /// アップデートボタン: 設定のアップデート画面を表示する（設定が未ドッキングならドッキングする）。
    fn open_update(&mut self, cx: &mut Context<Self>) {
        updater::reveal(cx);
        match ui::docked(cx).filter(|_| self.current(cx) == Some(Page::Settings)) {
            Some(settings) => ui::redraw(settings, cx),
            None => self.toggle(Page::Settings, cx),
        }
    }

    /// 表示中にどのパネルにもヒットしない空領域を押下した場合に非表示にする。
    /// フレームループが状態を書き戻すたびに呼ばれるため、変化がなければ何もしない（書き込むと自身が再度呼ばれる）。
    fn close_on_outside_press(&mut self, cx: &mut Context<Self>) {
        let Some(status) = cx
            .global::<PanelControl>()
            .panels
            .get(&self.panel)
            .map(|p| &p.status)
        else {
            return;
        };
        let (count, hidden) = (status.outside_press, status.hidden);
        // 前回の確認時も表示中だった場合のみ反応。非表示中（再表示と同一フレームを含む）の押下は無視し、
        // 表示直後に非表示化されるのを防ぐ（開始音だけ再生されて表示されない状態を回避）。
        let seen = self.outside.replace((count, hidden));
        if seen.is_some_and(|(seen, was_hidden)| seen != count && !was_hidden) && !hidden {
            hide(self.panel, cx);
        }
    }

    /// ダッシュボードの距離・高さ設定が変更されたら、表示中であれば現在の正面に再配置する（非表示中なら次回の [`show`] で反映される）。
    /// フレームループが状態を書き戻すたびに呼ばれるため、変化がなければ何もしない。
    fn follow_placement(&mut self, cx: &mut Context<Self>) {
        let s = ui::settings(cx);
        let now = (s.dashboard_distance, s.dashboard_height);
        let hidden = cx
            .global::<PanelControl>()
            .panels
            .get(&self.panel)
            .is_none_or(|p| p.status.hidden);
        if self.placement.replace(now).is_some_and(|seen| seen != now) && !hidden {
            commands(self.panel, [PanelCommand::Place(placed(cx))], cx);
        }
    }

    /// `panel` に `page` をドッキングした記録を保持する。
    fn track(&mut self, page: Page, panel: PanelId, cx: &mut Context<Self>) {
        self.pages.retain(|&(p, _)| p != page);
        self.pages.push((page, panel));
        ui::notify(self.panel, cx);
    }
}

/// バーに並べる配信ボタンの最大数（あふれる場合は最後の 1 つを、残りを一覧表示するボタンに充てる）。
const MAX_STREAM_BUTTONS: usize = 10;

/// 配信が `n` 個あるとき、バーにボタンとして並べる個数。
fn shown_streams(n: usize) -> usize {
    if n > MAX_STREAM_BUTTONS {
        MAX_STREAM_BUTTONS - 1
    } else {
        n
    }
}

/// 開いているポップオーバー（外側押下でクローズ済みなら `None`）。
fn open_popover(id: Option<PanelId>, cx: &App) -> Option<PanelId> {
    id.filter(|id| {
        !cx.global::<PanelControl>()
            .panels
            .get(id)
            .is_some_and(|p| p.status.closed)
    })
}

pub(crate) fn stream_key(panel: PanelId) -> SharedString {
    format!("dashboard-stream-{panel}").into()
}

/// バーの最大幅（px）。ウィンドウ幅（[`BAR_PX`]）からシャドウ余白を差し引いた値。配信等が蓄積してこの幅を超える場合、横スクロールする。
const MAX_BAR_PX: f32 = BAR_PX.0 as f32 - 2.0 * components::PANEL_MARGIN;

/// 区切り線の自然な幅（線と両脇の余白）。
const SEPARATOR_W: f32 = 1.0 + 2.0 * spacing::XS;

/// クイック設定ボタンの自然幅（px）: 左右の余白＋時刻、通知があれば件数表示（ベルアイコン＋数字幅 `count`＋間隔）を加算。
fn quick_settings_width(time_width: f32, count: Option<f32>) -> f32 {
    2.0 * spacing::XL
        + time_width
        + count.map_or(0.0, |w| spacing::SM + font_size::LG + spacing::XS + w)
}

/// ダッシュボードで開いた配信。
#[derive(Clone)]
pub(crate) struct Stream {
    pub(crate) panel: PanelId,
    kind: SourceKind,
    name: SharedString,
    /// ウィンドウのアプリアイコン（取得できなければウィンドウアイコンで代替）。
    icon: Option<Arc<RenderImage>>,
    /// 他の配信を選択したため非表示にして取り外した状態。
    pub(crate) stashed: bool,
}

/// バーの区切り線の高さ（px）。
const SEPARATOR_PX: f32 = 32.0;

fn separator(theme: &Theme) -> Div {
    // 丸いボタン同士は縁が離れるため広く見える。直線の両側のマージンを広めに取り、視覚的な間隔を統一する。
    div()
        .flex_none()
        .mx(px(spacing::XS))
        .w(px(1.0))
        .h(px(SEPARATOR_PX))
        .bg(rgba(theme.neutral_border_subtle))
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // フレームループより先に描画される場合があるため、受け渡し口がなければ生成する。
        cx.default_global::<PanelControl>();
        self.prune_streams(cx);
        let time_width = clock_width(window, font_size::MD);
        let (mut enter, now_at) = (std::mem::take(&mut self.enter), design::drive::now());
        let hidden = cx
            .global::<PanelControl>()
            .panels
            .get(&self.panel)
            .is_some_and(|p| p.status.hidden);
        enter.begin(hidden, now_at);
        // ボタンの左からの配置順: メニュー 0・クイック設定 1・再生中・ランチャー・アップデート・区切り線＋配信。
        let streams_now = with_leaving(
            self.streams.clone(),
            &self.prev_streams,
            |s| stream_key(s.panel),
            &mut enter,
            now_at,
        );
        self.prev_streams = streams_now.iter().map(|(s, _)| s.clone()).collect();
        let has_media = cx.global::<PanelControl>().media.is_some();
        let media_key = SharedString::from("dashboard-now-playing");
        let media_exit = !has_media && self.prev_media && enter.start_exit(&media_key, now_at);
        self.prev_media = has_media || media_exit;
        let media = usize::from(self.prev_media);
        let sep_key = SharedString::from("dashboard-separator");
        let sep_now = streams_now.iter().any(|(_, leaving)| !leaving);
        let sep_exit = !sep_now && self.prev_sep && enter.start_exit(&sep_key, now_at);
        self.prev_sep = sep_now || sep_exit;
        let has_update = updater::available(cx);
        let update_key = SharedString::from("dashboard-update");
        let update_exit = !has_update && self.prev_update && enter.start_exit(&update_key, now_at);
        self.prev_update = has_update || update_exit;
        let update = usize::from(self.prev_update);
        let stream_slot = 3 + media + update;
        let theme = Theme::current(cx);
        let current = self.current(cx);
        let docked = ui::docked(cx);
        // 並べきれない配信は末尾のボタンの一覧ポップオーバーにまとめる（消えていく最中のものは、滑り出るため並べたままにする）。
        let (mut live, shown) = (0, shown_streams(self.streams.len()));
        let (visible, _): (Vec<_>, Vec<_>) = streams_now.iter().partition(|(_, leaving)| {
            *leaving || {
                live += 1;
                live <= shown
            }
        });
        let streams: Vec<_> = visible
            .into_iter()
            .enumerate()
            .map(|(i, (s, leaving))| {
                let id = s.panel;
                let variant = if docked == Some(id) {
                    Variant::Primary
                } else {
                    Variant::Default
                };
                let glyph = match (s.kind, &s.icon) {
                    (SourceKind::Window, Some(icon)) => {
                        img(icon.clone()).size(px(font_size::LG)).into_any_element()
                    }
                    (SourceKind::Window, None) => design::icon(&icons::APP_WINDOW)
                        .weight(design::Weight::Bold)
                        .size(px(font_size::LG))
                        .into_any_element(),
                    (SourceKind::Screen, _) => design::icon(&icons::MONITOR)
                        .weight(design::Weight::Bold)
                        .size(px(font_size::LG))
                        .into_any_element(),
                };
                let key = stream_key(id);
                let t = if *leaving {
                    enter.progress_out(&key, now_at)
                } else {
                    enter.progress(&key, stream_slot + 1 + i, now_at)
                };
                slide_in(
                    t,
                    components::BUTTON_HEIGHT,
                    components::round_button(&theme, key, s.name.clone(), variant)
                        .flex_none()
                        .child(glyph)
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_stream(id, cx))),
                )
            })
            .collect();
        let overflow_docked = self.overflow().iter().any(|s| docked == Some(s.panel));
        let overflow_button = (!self.overflow().is_empty()).then(|| {
            let key = SharedString::from("dashboard-stream-overflow");
            let t = enter.progress(&key, stream_slot + 1 + shown, now_at);
            slide_in(
                t,
                components::BUTTON_HEIGHT,
                components::icon_button(
                    &theme,
                    key,
                    &icons::DOTS_THREE,
                    i18n::t(cx, "dashboard.streams"),
                    if overflow_docked || open_popover(self.overflow_menu, cx).is_some() {
                        Variant::Primary
                    } else {
                        Variant::Default
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_overflow(cx))),
            )
        });
        let variant = |page| {
            if current == Some(page) {
                Variant::Primary
            } else {
                Variant::Default
            }
        };
        // 時刻の横に未読通知の件数を表示する（一覧はクイック設定。開くと既読になる）。0 件なら非表示。
        let count = cx
            .global::<PanelControl>()
            .notifications
            .iter()
            .filter(|n| !ui::notification::is_read(n.id, cx))
            .count();
        let count_width = (count > 0).then(|| {
            f32::from(design::font::width(
                count.to_string(),
                font_size::MD,
                FontWeight::BOLD,
                window,
            ))
        });
        let page_button = |id, glyph, tooltip, page| {
            components::icon_button(&theme, id, glyph, tooltip, variant(page))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(page, cx)))
        };
        let mirror = page_button(
            "dashboard-launcher",
            &icons::DOTS_NINE,
            i18n::t(cx, "dashboard.launcher"),
            Page::Launcher,
        );
        let now_playing = page_button(
            "dashboard-now-playing",
            &icons::PLAYLIST,
            i18n::t(cx, "dashboard.now_playing"),
            Page::NowPlaying,
        );
        let t = if has_media {
            enter.progress(&media_key, 2, now_at)
        } else if media_exit {
            enter.progress_out(&media_key, now_at)
        } else {
            Slide::SHOWN
        };
        let now_playing = slide_in(t, components::BUTTON_HEIGHT, now_playing);
        let t = enter.progress(&"dashboard-launcher".into(), 2 + media, now_at);
        let mirror = slide_in(t, components::BUTTON_HEIGHT, mirror);
        // 新規リリースがある場合のみ、ランチャーの右に強調色のボタンを表示。押下で設定のアップデート画面を開く。
        let t = if has_update {
            enter.progress(&update_key, 3 + media, now_at)
        } else if update_exit {
            enter.progress_out(&update_key, now_at)
        } else {
            Slide::SHOWN
        };
        let update_button = slide_in(
            t,
            components::BUTTON_HEIGHT,
            components::icon_button(
                &theme,
                update_key.clone(),
                &icons::ARROW_CIRCLE_UP,
                i18n::t(cx, "dashboard.update"),
                Variant::Primary,
            )
            .on_click(cx.listener(|this, _, _, cx| this.open_update(cx))),
        );
        let menu_t = enter.progress(&"dashboard-menu".into(), 0, now_at);
        let quick_t = enter.progress(&"dashboard-quick-settings".into(), 1, now_at);
        let quick_w = quick_settings_width(f32::from(time_width), count_width);
        // 区切り線はスライドインせず、透明度と幅のみ変化する。
        let separator_t = if sep_now {
            enter.progress(&sep_key, stream_slot, now_at)
        } else if sep_exit {
            enter.progress_out(&sep_key, now_at)
        } else {
            Slide::SHOWN
        };
        // ボタンのアニメーション中は継続的に再描画する。
        self.enter = enter;
        let scrolling = self.enter.follow_scroll(&self.scroll, now_at);
        if self.enter.finish() || scrolling {
            components::keep_drawing(window, cx);
        }
        let menu = components::icon_button(
            &theme,
            "dashboard-menu",
            &icons::LIST,
            i18n::t(cx, "dashboard.menu"),
            if self.open_menu(cx).is_some() {
                Variant::Primary
            } else {
                Variant::Default
            },
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(cx)));
        let menu = slide_in(menu_t, components::BUTTON_HEIGHT, menu);
        let quick = slide_in(
            quick_t,
            quick_w,
            components::content_button(
                &theme,
                "dashboard-quick-settings",
                variant(Page::QuickSettings),
            )
            .px(px(spacing::XL))
            // 数字による幅変動を防ぐため、時刻表示の幅を固定する。
            .child(fixed_clock(time_width))
            .tooltip(i18n::t(cx, "dashboard.quick_settings"))
            .children(count_width.map(|_| {
                div()
                    .debug_selector(|| "dashboard-notification-count".into())
                    .flex()
                    .items_center()
                    .gap(px(spacing::XS))
                    .child(design::icon(&icons::BELL).size(px(font_size::LG)))
                    .child(div().font_weight(FontWeight::BOLD).child(count.to_string()))
            }))
            .on_click(cx.listener(|this, _, _, cx| this.toggle(Page::QuickSettings, cx))),
        );
        // 左から: メニュー・クイック設定・再生中（メディア再生時のみ）・ランチャー・アップデート（新規リリース時のみ）・
        // 区切り線＋開いた配信。
        let mut items: Vec<AnyElement> = vec![menu.into_any_element(), quick.into_any_element()];
        if self.prev_media {
            items.push(now_playing.into_any_element());
        }
        items.push(mirror.into_any_element());
        if self.prev_update {
            items.push(update_button.into_any_element());
        }
        if self.prev_sep {
            items.push(fade_in(separator_t, SEPARATOR_W, separator(&theme)).into_any_element());
        }
        items.extend(streams.into_iter().map(IntoElement::into_any_element));
        items.extend(overflow_button.map(IntoElement::into_any_element));
        // ウィンドウは固定幅だが、バーは内容幅に収めて中央配置する（[`MAX_BAR_PX`] を超える場合は上限固定で横スクロール）。
        div()
            .absolute()
            .inset(px(components::PANEL_MARGIN))
            .flex()
            .justify_center()
            .child(
                fit_bar(
                    &theme,
                    "dashboard-bar",
                    &self.scroll,
                    self.enter.peak(),
                    Some(MAX_BAR_PX),
                    items,
                )
                .debug_selector(|| "dashboard-bar".into()),
            )
    }
}

#[cfg(test)]
mod tests;
