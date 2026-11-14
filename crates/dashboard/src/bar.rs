//! バー（[`Dashboard`]）: メニュー・クイック設定・再生中・ニャプリの作業・ランチャー・アップデートのボタンと、開いた配信のボタン。
//! 押すと画面をはめ、もう一度押すと閉じる。配信は溜まっていき、選ぶとはまっていたものは隠して外す。

use super::*;

/// 進行中の作業の知らせ（ライブアクティビティ。丸いボタン）の、進み具合の輪の一辺と太さ・中のアイコンの一辺・
/// 長い文字（`時:分:秒`）の大きさ（px）。
const ACTIVITY_RING_PX: f32 = 48.0;
const ACTIVITY_RING_STROKE: f32 = 4.0;
const ACTIVITY_ICON_PX: f32 = 28.0;
const ACTIVITY_SMALL_TEXT_PX: f32 = 11.0;

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
        // 新しいリリースがあるかは起動時と、常駐して開きっぱなしでも分かるよう周期で確かめる（あればバーにボタンが出る。
        // 入れるのはユーザーが選んだときだけ）。開発中のビルドとテストは確かめない（設定から押せる）。
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
        // アプリ一覧は取得に数秒かかるので、起動時に読んでおく（ランチャーを開いたときにはもう出せる）。
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
        // 画面（モニター）の一覧が届いたら、すべて配信として開いておく。隠している間は描かないので、描画とは別に待つ。
        // テストでは読まない（OS の一覧が届く時刻がテストの待ち合わせと競合し、配信が開くかどうかが毎回変わる）。
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
            shown: None,
            outside: None,
            placement: None,
            prev_streams: Vec::new(),
            prev_activities: Vec::new(),
            prev_media: false,
            prev_update: false,
            prev_sep: false,
            scroll: ScrollHandle::new(),
            enter: Entering::new(cfg!(test)),
        }
    }

    /// 開いているメニューのポップオーバー（外側を押して閉じていれば `None`）。
    fn open_menu(&self, cx: &App) -> Option<PanelId> {
        self.menu.filter(|id| {
            !cx.global::<PanelControl>()
                .panels
                .get(id)
                .is_some_and(|p| p.status.closed)
        })
    }

    /// メニューを開く・閉じる。開いている間にボタンを押すと、外側を押したことになってオーバーレイが閉じる。
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
            // バーは中身の幅で中央に置かれ、幅は配信などで変わる。開く今のバーの左端に合わせる。
            .offset(menu_offset(f32::from(self.scroll.bounds().left())))
            .without_resize();
            cx.default_global::<PanelControl>().spawns.push(spec);
        }
        ui::notify(self.panel, cx);
    }

    /// 配信 `panel` を開いた（はめる・隠すのは開く側。`resident` なら常駐のもので、隠して開いた）。バーにボタンが増える。
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

    /// 画面（モニター）をすべて、常駐の配信として隠して開いておく（バーのボタンで出せる。閉じずにしまう）。
    /// もう映しているものは開かない。
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

    /// はまっている配信を、閉じずに隠して外す（ボタンを押せばまた出せる）。配信でなければ何もしない
    /// （ほかの画面は、次にはめる画面と入れ替わりに閉じる）。
    fn stash_docked(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = ui::docked(cx) {
            self.stash(id, cx);
        }
    }

    /// 常駐の配信 `id` をしまう（ナビゲーションバーのしまうボタン）。はまっていても独立させていても、隠してバーに残す。
    pub(crate) fn minimize(&mut self, id: PanelId, cx: &mut Context<Self>) {
        self.stash(id, cx);
        ui::notify(self.panel, cx);
    }

    /// 配信 `id` を、閉じずに隠して外す。配信でなければ何もしない。
    fn stash(&mut self, id: PanelId, cx: &mut Context<Self>) {
        let Some(stream) = self.streams.iter_mut().find(|s| s.panel == id) else {
            return;
        };
        stream.stashed = true;
        // 隠した配信へは打てないので、そこへ打つキーボードも片付ける。
        if cx
            .try_global::<ui::keyboard::InputRouting>()
            .and_then(|r| r.active)
            .is_some_and(|s| s.target == id)
        {
            ui::keyboard::dismiss_keyboard(cx);
        }
        // 先に隠してから外す（見えたまま外すと、離れる動きが見えてしまう）。
        commands(id, [PanelCommand::Hide(true)], cx);
        ui::undock(id, cx);
    }

    /// 配信 `id` をはめる（はまっていた配信は隠し、ほかの画面は閉じる）。もうはまっていれば隠す。
    pub fn toggle_stream(&mut self, id: PanelId, cx: &mut Context<Self>) {
        let previous = ui::docked(cx);
        let docked = previous == Some(id);
        // 配信から配信へ切り替えるなら、出ている OS キーボードは閉じずに次の配信へ譲る（隠す配信と一緒に片付けない）。
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
            // 隠したままはめてから出す。先に出すと、隠した時の場所（その後ダッシュボードを動かしていれば遠く）から
            // はめる場所へ滑って・回って入ってくる。
            ui::dock(id, cx);
            // 出したウィンドウを OS で前に出す（そのまま操作できるように）。
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

    /// 配信 `id` がダッシュボードの配信で、はまっていなければはめる（独立させたものは動かさない）。
    pub(crate) fn show_stream(&mut self, id: PanelId, cx: &mut Context<Self>) {
        if ui::docked(cx) != Some(id) && self.streams.iter().any(|s| s.panel == id) {
            self.toggle_stream(id, cx);
        }
    }

    /// 閉じた配信のボタンを消す。独立させたものは残す（ボタンで戻せる）。
    fn prune_streams(&mut self, cx: &App) {
        let ctl = cx.global::<PanelControl>();
        self.streams
            .retain(|s| !ctl.panels.get(&s.panel).is_some_and(|p| p.status.closed));
    }

    /// 今はまっている画面。
    pub fn current(&self, cx: &App) -> Option<Page> {
        let docked = ui::docked(cx)?;
        self.shown
            .filter(|&(_, id)| id == docked)
            .map(|(page, _)| page)
    }

    /// `page` の画面をはめる（はまっていた画面は閉じる）。もうはまっていれば閉じる。
    pub fn toggle(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.current(cx) == Some(page) {
            if let Some(id) = ui::docked(cx) {
                close(id, cx);
            }
            self.shown = None;
        } else {
            // はまっている配信は閉じずに隠す（ほかの画面は、次の画面と入れ替わりに閉じる）。
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
            // はめる画面は、開いた元（バー）の手前ではなく、はめる場所に合わせて目の前に出す。
            cx.default_global::<PanelControl>()
                .spawns
                .push(spec.fit_height(SIZE_M.y).in_view(SCREEN_POSE).dockable());
        }
        ui::notify(self.panel, cx);
    }

    /// ニャプリ `app` をはめる（はまっていた画面は閉じ、配信は隠す）。もう開いていれば、そのパネルをはめる。
    pub fn open_nyapp(&mut self, app: nyapp::Installed, cx: &mut Context<Self>) {
        self.stash_docked(cx);
        self.shown = None;
        if let Some(panel) = nyapp::panel_of(&app.manifest.id, cx) {
            ui::dock(panel, cx);
            commands(panel, [PanelCommand::Hide(false)], cx);
        } else {
            let spec = nyapp::spec(self.panel, app, i18n::Lang::current(cx), |id, cx| {
                ui::dock(id, cx)
            });
            // はめる画面は、開いた元（ランチャー）の手前ではなく、はめる場所に合わせて目の前に出す。
            cx.default_global::<PanelControl>()
                .spawns
                .push(spec.fit_height(SIZE_M.y).in_view(SCREEN_POSE).dockable());
        }
        ui::notify(self.panel, cx);
    }

    /// アップデートのボタン: 設定のアップデートを出す（設定がはまっていなければはめる）。
    fn open_update(&mut self, cx: &mut Context<Self>) {
        updater::reveal(cx);
        match ui::docked(cx).filter(|_| self.current(cx) == Some(Page::Settings)) {
            Some(settings) => ui::redraw(settings, cx),
            None => self.toggle(Page::Settings, cx),
        }
    }

    /// 進行中の作業の知らせを押した: 出したニャプリを開く。終わったカウントダウンは知らせを片付ける。
    fn open_activity(&mut self, live: nyapp::activity::Live, cx: &mut Context<Self>) {
        if live.finished {
            nyapp::activity::clear(&live.app.manifest.id, cx);
        }
        self.open_nyapp(live.app, cx);
    }

    /// 出ている間に何も無い所（ダッシュボードにも、ほかのパネルにも当たらない所）を押したら隠す。
    /// フレームループが状態を書き戻すたびに呼ばれるので、変わっていなければ何もしない（書き込むと自分がまた呼ばれる）。
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
        // 前に見たときも出ていたときだけ。隠している間（出し直したのと同じフレームを含む）に押した分で、
        // 出した直後に隠さない（開く音だけ鳴って出ない）。
        let seen = self.outside.replace((count, hidden));
        if seen.is_some_and(|(seen, was_hidden)| seen != count && !was_hidden) && !hidden {
            hide(self.panel, cx);
        }
    }

    /// 設定のダッシュボードの距離・高さが変わったら、出ていれば今の目の前に置き直す（隠していれば次に出すとき [`show`] で効く）。
    /// フレームループが状態を書き戻すたびに呼ばれるので、変わっていなければ何もしない。
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

    /// `panel` に `page` をはめた。その画面のシステムのニャプリとして覚える。
    fn track(&mut self, page: Page, panel: PanelId, cx: &mut Context<Self>) {
        let system = match page {
            Page::Launcher => nyapp::system::LAUNCHER,
            Page::Debug => nyapp::system::DEBUG,
            Page::Settings => nyapp::system::SETTINGS,
            Page::QuickSettings | Page::NowPlaying => nyapp::system::DASHBOARD,
        };
        nyapp::opened(system.id, panel, cx);
        self.shown = Some((page, panel));
        ui::notify(self.panel, cx);
    }
}

fn stream_key(panel: PanelId) -> SharedString {
    format!("dashboard-stream-{panel}").into()
}

fn activity_key(live: &nyapp::activity::Live) -> SharedString {
    format!("dashboard-activity-{}", live.app.manifest.id).into()
}

/// バーの最大幅（px）。窓の幅（[`BAR_PX`]）から影の余白を引いたもので、配信などが溜まってこれを超えそうなら、横にスクロールする。
const MAX_BAR_PX: f32 = BAR_PX.0 as f32 - 2.0 * components::PANEL_MARGIN;

/// 区切り線の自然な幅（線と両脇の余白）。
const SEPARATOR_W: f32 = 1.0 + 2.0 * spacing::XS;

/// クイック設定のボタンの自然な幅（px）: 左右の余白と時刻、電池があればその記号（と隙間）。
fn quick_settings_width(time_width: f32, battery: bool) -> f32 {
    2.0 * spacing::XL
        + time_width
        + if battery {
            spacing::SM + font_size::LG
        } else {
            0.0
        }
}

/// ダッシュボードで開いた配信。
#[derive(Clone)]
pub(crate) struct Stream {
    pub(crate) panel: PanelId,
    kind: SourceKind,
    name: SharedString,
    /// ウィンドウのアプリのアイコン（無ければウィンドウの記号）。
    icon: Option<Arc<RenderImage>>,
    /// ほかを選んだので隠して外している。
    pub(crate) stashed: bool,
}

/// バーの区切り線の高さ（px）。
const SEPARATOR_PX: f32 = 32.0;

fn separator(theme: &Theme) -> Div {
    // 丸いボタンどうしは縁が離れていくぶん広く見えるので、まっすぐな線の両脇は少し広く取って、見た目の間隔をそろえる。
    div()
        .flex_none()
        .mx(px(spacing::XS))
        .w(px(1.0))
        .h(px(SEPARATOR_PX))
        .bg(rgba(theme.neutral_border_subtle))
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // フレームループより先に描くこともあるので、受け渡し口が無ければ作る。
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
        // 左から数えたボタンの順番: メニュー 0・クイック設定 1・進行中の作業・再生中・ランチャー・アップデート・区切り線と配信。
        let activities_now = with_leaving(
            nyapp::activity::all(cx).to_vec(),
            &self.prev_activities,
            activity_key,
            &mut enter,
            now_at,
        );
        self.prev_activities = activities_now.iter().map(|(l, _)| l.clone()).collect();
        let streams_now = with_leaving(
            self.streams.clone(),
            &self.prev_streams,
            |s| stream_key(s.panel),
            &mut enter,
            now_at,
        );
        self.prev_streams = streams_now.iter().map(|(s, _)| s.clone()).collect();
        let n_activity = activities_now.len();
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
        let stream_slot = 3 + n_activity + media + update;
        let theme = Theme::current(cx);
        let current = self.current(cx);
        let docked = ui::docked(cx);
        let streams: Vec<_> = streams_now
            .iter()
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
        let variant = |page| {
            if current == Some(page) {
                Variant::Primary
            } else {
                Variant::Default
            }
        };
        // 時刻の横の電池は HMD のもの（無ければ最初のデバイス）。
        let ctl = cx.global::<PanelControl>();
        let battery = ctl
            .batteries
            .iter()
            .find(|b| b.device == Device::Hmd)
            .or(ctl.batteries.first())
            .cloned();
        let page_button = |id, glyph, tooltip, page| {
            components::icon_button(&theme, id, glyph, tooltip, variant(page))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(page, cx)))
        };
        // 進行中の作業の知らせ（Dynamic Island のような進み具合の表示）: 1:1 の丸いボタン。進み具合があれば円の輪で示し、
        // その中に残り時間などの文字。終わったものと文字の無いものは、アプリのアイコンの前景。
        let now = nyapp::activity::now_ms();
        let lang = i18n::Lang::current(cx).code();
        let activities: Vec<_> = activities_now
            .into_iter()
            .enumerate()
            .map(|(i, (live, leaving))| {
                let text = live.text(now);
                let tooltip = match &text {
                    Some(t) => format!("{} · {t}", live.activity.title()),
                    None => format!(
                        "{} · {}",
                        live.activity.title(),
                        i18n::t(cx, "dashboard.activity_done")
                    ),
                };
                let middle = match &text {
                    Some(t) if !live.finished => div()
                        .text_size(px(if t.chars().count() > 5 {
                            ACTIVITY_SMALL_TEXT_PX
                        } else {
                            font_size::SM
                        }))
                        .font_weight(FontWeight::BOLD)
                        .child(t.clone())
                        .into_any_element(),
                    _ => live.app.foreground(lang, ACTIVITY_ICON_PX, &theme),
                };
                let key = activity_key(&live);
                let t = if leaving {
                    enter.progress_out(&key, now_at)
                } else {
                    enter.progress(&key, 2 + i, now_at)
                };
                let button = components::round_button(&theme, key, tooltip, Variant::Default)
                    .flex_none()
                    .relative()
                    .children(live.progress(now).map(|p| {
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(components::progress_ring(
                                &theme,
                                ACTIVITY_RING_PX,
                                ACTIVITY_RING_STROKE,
                                p,
                            ))
                    }))
                    .child(middle)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.open_activity(live.clone(), cx)),
                    );
                slide_in(t, components::BUTTON_HEIGHT, button)
            })
            .collect();
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
            enter.progress(&media_key, 2 + n_activity, now_at)
        } else if media_exit {
            enter.progress_out(&media_key, now_at)
        } else {
            Slide::SHOWN
        };
        let now_playing = slide_in(t, components::BUTTON_HEIGHT, now_playing);
        let t = enter.progress(&"dashboard-launcher".into(), 2 + n_activity + media, now_at);
        let mirror = slide_in(t, components::BUTTON_HEIGHT, mirror);
        // 新しいリリースがあるときだけ、ランチャーの右に強調色のボタン。押すと設定のアップデートを出す。
        let t = if has_update {
            enter.progress(&update_key, 3 + n_activity + media, now_at)
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
        let quick_w = quick_settings_width(f32::from(time_width), battery.is_some());
        // 区切り線は滑り込まず、透明度と幅だけが変わる。
        let separator_t = if sep_now {
            enter.progress(&sep_key, stream_slot, now_at)
        } else if sep_exit {
            enter.progress_out(&sep_key, now_at)
        } else {
            Slide::SHOWN
        };
        // 出ているボタンが動いている間は、次のフレームも描く。
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
            // 数字で幅が変わって落ち着かないので、時刻の幅を固定する。
            .child(fixed_clock(time_width))
            .tooltip(i18n::t(cx, "dashboard.quick_settings"))
            .children(battery.map(|b| design::icon(battery_glyph(&b)).size(px(font_size::LG))))
            .on_click(cx.listener(|this, _, _, cx| this.toggle(Page::QuickSettings, cx))),
        );
        // 左から: メニュー・クイック設定・進行中の作業（クイック設定と再生中の間）・再生中（何か再生しているときだけ）・
        // ランチャー・アップデート（新しいリリースがあるときだけ）・区切り線と、開いた配信。
        let mut items: Vec<AnyElement> = vec![menu.into_any_element(), quick.into_any_element()];
        items.extend(activities.into_iter().map(IntoElement::into_any_element));
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
        // 窓は固定の幅だが、バーは中身の幅に収めて中央に置く（[`MAX_BAR_PX`] を超えそうなら、そこで頭打ちにして横にスクロール）。
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
