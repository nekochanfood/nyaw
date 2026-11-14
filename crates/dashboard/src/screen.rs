//! ダッシュボード自身の画面（[`Screen`]）: クイック設定・ランチャー・再生中。中身はそれぞれのファイル
//! （`quick_settings.rs` / `launcher.rs` / `now_playing.rs`）の `impl Screen` にある。ここは作り方と、取得元の一覧の見張りだけ。

use super::*;

/// クイック設定・ランチャー・再生中の起動仕様（`page` はそのどれか）。開くと `opened` でダッシュボードにはまる。
/// 閉じるのはナビゲーションバーかバーの同じボタン。
pub(crate) fn screen_spec(
    from: PanelId,
    page: Page,
    dashboard: WeakEntity<Dashboard>,
    opened: Opened,
) -> PanelSpec {
    let title = match page {
        Page::Launcher => "dashboard.launcher",
        Page::NowPlaying => "dashboard.now_playing",
        _ => "dashboard.quick_settings",
    };
    PanelSpec::spawn(from, move |id, _, cx: &mut Context<Screen>| {
        opened(id, cx);
        Screen::new(id, page, dashboard, cx)
    })
    .sized(SIZE_M, RES_PX)
    .titled(title)
}

impl Screen {
    pub(crate) fn new(
        panel: PanelId,
        page: Page,
        dashboard: WeakEntity<Dashboard>,
        cx: &mut Context<Self>,
    ) -> Self {
        // 時計・電池・再生位置は 1 秒ごとに描き直す。ランチャーは取得元の到着を待つ間だけ。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let tick = |s: &mut Self, cx: &mut Context<Self>| {
                    if s.page != Page::Launcher || s.pending.is_some() {
                        ui::notify(panel, cx);
                    }
                };
                if this.update(cx, tick).is_err() {
                    return;
                }
            }
        })
        .detach();
        let mut this = Self {
            panel,
            page,
            dashboard,
            sources: Vec::new(),
            pending: None,
            error: None,
            list: ListState::new(0, ListAlignment::Top, px(2.0 * TILE_PX)),
            launcher: Launcher::default(),
            media_images: None,
            open_app: false,
            apps: cx
                .try_global::<AppCatalog>()
                .map(|c| c.0.clone())
                .unwrap_or_default(),
            apps_pending: false,
            query: String::new(),
            search: cx.focus_handle(),
            nyapps: Vec::new(),
            nyapps_pending: false,
            removing: false,
            update_note: None,
        };
        // 再生中の画面は、アプリのアイコンが OS から取れないとき（デスクトップのアプリ）にウィンドウのものを使う。
        if matches!(page, Page::Launcher | Page::NowPlaying) {
            this.refresh();
        }
        if page == Page::Launcher {
            this.apps_pending = true;
            let fetch = fetch_apps(cx);
            cx.spawn(async move |this, cx| {
                let fetched = fetch.await;
                let _ = this.update(cx, |s, cx| {
                    match fetched {
                        Ok(apps) => {
                            cx.set_global(AppCatalog(apps.clone()));
                            s.apps = apps;
                        }
                        Err(e) => s.error = Some(e),
                    }
                    s.apps_pending = false;
                    ui::notify(s.panel, cx);
                });
            })
            .detach();
            this.scan_nyapps(cx);
        }
        this
    }

    /// 取得元を取り直す（取得は別スレッド。届くまでは 1 秒ごとの描き直しで見に来る）。
    pub(crate) fn refresh(&mut self) {
        match Sources::refresh() {
            Ok(pending) => (self.pending, self.error) = (Some(pending), None),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// 取得元の一覧が届いていれば受け取る。
    pub(crate) fn poll_sources(&mut self) {
        let Some(result) = self.pending.as_ref().and_then(Sources::poll) else {
            return;
        };
        self.pending = None;
        match result {
            Ok(mut sources) => {
                sources.sort_by(|a, b| a.name.cmp(&b.name));
                self.sources = sources
                    .into_iter()
                    .map(|s| {
                        let image = s.icon.as_ref().and_then(render_image);
                        (s, image)
                    })
                    .collect();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

impl Render for Screen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        cx.default_global::<PanelControl>();
        let theme = Theme::current(cx);
        match self.page {
            Page::Launcher => self.apps_page(&theme, cx),
            Page::NowPlaying => self.now_playing(cx),
            // デバッグ・設定は main が渡す別の画面で、Screen にはならない。
            _ => {
                let width = clock_width(window, QUICK_TIME_PX);
                self.quick_settings(&theme, width, cx)
            }
        }
    }
}
