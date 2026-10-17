//! Layer 6 画面: ダッシュボード。左手の X ボタンを二回続けて押すと開き、もう一度二回押すと閉じる
//! （開け閉めは常駐の wrist が [`kernel::PanelStatus::double_press`] を見て行う）。開いている間は空間全体が暗くなる。
//! ダッシュボードは少し上を向けたバー（[`Dashboard`]）そのもので、開いた時は何も出さない。バーのボタンで、
//! クイック設定・画面のミラーリング（[`Screen`]）・デバッグ・設定の画面（それぞれ自分のナビゲーションバーを持つ別のパネル）を
//! バーの上にはめる（`ui::dock`。同じボタンをもう一度押すと閉じる）。ミラーリングで選んだ画面・ウィンドウもそこにはまる。
//! はめた画面はダッシュボードに固定され（一緒に動き、掴むとダッシュボードごと動く）、ナビゲーションバーから外して
//! 独立させたり、戻したりできる。閉じる（バーの X・X ボタンの二回押し）と、はめた画面ごと隠すだけで、次は同じ状態で出る。

use std::sync::Arc;
use std::time::Duration;

use design::components::{self, Variant};
use design::{Theme, font_size, icons, spacing};
use gpui::{
    App, Context, Div, FontWeight, RenderImage, ScrollHandle, WeakEntity, Window, div, img,
    prelude::*, px, rgba,
};
use kernel::audio::Sound;
use kernel::desktop::{DEFAULT_FRAME_RATE, Icon, Source, SourceKind, Sources, Status};
use kernel::vr::{Battery, Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId, Setting};
use ui::{PanelControl, PanelSpec};

/// バーを手前に傾ける角度（下から見上げずに読めるよう、少し上を向かせる）。はめた画面はこの分だけ起こして垂直に立てる。
const BAR_TILT: f32 = std::f32::consts::FRAC_PI_6;
/// はめた画面の中心までの距離（m）。画面は目の高さの正面に垂直に立ち、バーはその下に来る（[`pose`]）。
const DISTANCE: f32 = 1.1;
/// はめる画面の大きさと解像度（main が渡す設定画面と同じ）。ほかから渡される画面も、縦をこの高さに揃える。
const SIZE_M: Vec2 = Vec2::new(1.024, 0.768);
const RES_PX: (u32, u32) = (1024, 768);
/// バーの窓（px）。帯（ナビゲーションバーと同じ高さ）の周りに影の余白を取る。幅は画面と同じ密度。
const BAR_PX: (u32, u32) = (
    RES_PX.0,
    (components::NAV_BAR_HEIGHT + 2.0 * components::PANEL_MARGIN) as u32,
);
const BAR_M: Vec2 = Vec2::new(
    SIZE_M.x,
    SIZE_M.x * BAR_PX.1 as f32 / BAR_PX.0 as f32,
);
/// 画面を開いてからはまるまでの、最初の置き場所（はまる所）。
const SCREEN_POSE: Pose = Pose::new(Vec3::new(0.0, 0.0, -DISTANCE), Quat::IDENTITY);
/// グリッドの列の数とタイルの高さ（px）。
const COLUMNS: u16 = 4;
const TILE_PX: f32 = 120.0;
/// タイルのアイコンの大きさ（px）。
const TILE_ICON_PX: f32 = 48.0;
/// クイック設定の上の時刻の大きさ（px）。
const QUICK_TIME_PX: f32 = 48.0;
/// これより少ない残量は危険の色にする。
const LOW_BATTERY: f32 = 0.2;

/// 開くとき・閉じるときの音。どちらもダッシュボード自身から聞こえる（閉じる音は閉じ方を問わず鳴る）。
pub const OPEN_SOUND: &[u8] = include_bytes!("../assets/dashboard_open.wav");
pub const CLOSE_SOUND: &[u8] = include_bytes!("../assets/dashboard_close.wav");

/// ダッシュボードから開くほかの画面（デバッグ・設定）の起動仕様を作る。開いたら引数で PanelId を知らせる
/// （[`spec`] と同じ形）。画面どうしを直接つながないよう、何を開くかは main が選ぶ。
pub type Summon = fn(Box<dyn FnOnce(PanelId, &mut App)>) -> PanelSpec;

/// バーの置き場所（開いた瞬間の頭から見た相対姿勢）。はめた画面（[`SIZE_M`]）が目の高さの正面 [`DISTANCE`] に垂直に立つよう、
/// 画面の下半分・そのナビゲーションバー・隙間の分だけ下げ、傾けたバーの上端がその真下に来るように置く
/// （オーバーレイ層の `Parent::slot` の置き方の逆）。
pub fn pose() -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    let face = SIZE_M.y / 2.0 - margin(SIZE_M, RES_PX);
    // はめた画面のナビゲーションバーは、距離で大きさを変えない（1m 先での大きさのまま）。
    let nav_bar = ui::NAV_BAR_HEIGHT_M;
    let top = Vec3::new(0.0, -(face + nav_bar + kernel::overlay::SLOT_GAP), -DISTANCE);
    let rotation = Quat::from_rotation_x(-BAR_TILT);
    let half = BAR_M.y / 2.0 - margin(BAR_M, BAR_PX);
    Pose::new(top - rotation * Vec3::Y * half, rotation)
}

/// ダッシュボード（バー）の起動仕様。`opened` は作られたパネルの ID を受け取る。
pub fn spec(
    opened: impl FnOnce(PanelId, &mut App) + 'static,
    settings: Summon,
    debug: Summon,
) -> PanelSpec {
    PanelSpec::new(Pose::IDENTITY, move |id, _, cx: &mut Context<Dashboard>| {
        opened(id, cx);
        let ctl = cx.default_global::<PanelControl>();
        ctl.panels.entry(id).or_default().commands.extend([
            PanelCommand::Play(Sound::Wav(OPEN_SOUND), false),
            PanelCommand::CloseSound(Sound::Wav(CLOSE_SOUND)),
        ]);
        // バーの上を、画面をはめる場所にする。
        ctl.dock = Some((id, BAR_TILT));
        Dashboard::new(id, settings, debug, cx)
    })
    .sized(BAR_M, BAR_PX)
    .in_view(pose())
    // 閉じる（隠す）操作はバー自身の X にある。
    .without_nav_bar()
    // 大きさははめる画面に合わせて決めてあるので変えさせない。
    .without_resize()
    // 画面がはまっていなくても、画面が立つ所を基準に掴んで動かす（はめた画面を掴んだときと同じ動き）。
    .grab_anchor(pose().inverse() * SCREEN_POSE)
    .with_veil()
}

/// ダッシュボードを、はめた画面ごと隠す（閉じない。配信も止めない）。
pub fn hide(dashboard: PanelId, cx: &mut App) {
    commands(
        dashboard,
        [
            PanelCommand::Hide(true),
            PanelCommand::Play(Sound::Wav(CLOSE_SOUND), false),
        ],
        cx,
    );
}

/// 隠したダッシュボードを、今の目の前に同じ状態で出し直す。
pub fn show(dashboard: PanelId, cx: &mut App) {
    commands(
        dashboard,
        [
            PanelCommand::Place(pose()),
            PanelCommand::Hide(false),
            PanelCommand::Play(Sound::Wav(OPEN_SOUND), false),
        ],
        cx,
    );
}

/// ダッシュボードにはめる画面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    QuickSettings,
    Mirror,
    Debug,
    Settings,
}

/// ダッシュボード（バー）。左から、時刻と電池（クイック設定）・画面のミラーリング・デバッグ・設定、右端に閉じる（隠す）ボタン。
/// 今はまっている画面のボタンは強調色。
pub struct Dashboard {
    panel: PanelId,
    settings: Summon,
    debug: Summon,
    /// 最後にはめた画面（外されたり閉じられたりしていれば、[`Self::current`] は `None`）。
    shown: Option<(Page, PanelId)>,
    /// 反映した、何も無い所を押した回数（[`kernel::PanelStatus::outside_press`]）。最初に見た値から数える。
    outside: Option<u32>,
}

impl Dashboard {
    fn new(panel: PanelId, settings: Summon, debug: Summon, cx: &mut Context<Self>) -> Self {
        tick_every_second(panel, cx);
        cx.observe_global::<PanelControl>(Self::close_on_outside_press)
            .detach();
        Self {
            panel,
            settings,
            debug,
            shown: None,
            outside: None,
        }
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
            let dashboard = cx.weak_entity();
            let opened = Box::new(move |id, cx: &mut App| {
                ui::dock(id, cx);
                let _ = dashboard.update(cx, |d, cx| d.track(page, id, cx));
            });
            let spec = match page {
                Page::QuickSettings | Page::Mirror => screen_spec(page, cx.weak_entity(), opened),
                Page::Debug => (self.debug)(opened),
                Page::Settings => (self.settings)(opened),
            };
            cx.default_global::<PanelControl>()
                .spawns
                .push(spec.fit_height(SIZE_M.y).in_view(SCREEN_POSE).dockable());
        }
        redraw(self.panel, cx);
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
        if self.outside.replace(count).is_some_and(|seen| seen != count) && !hidden {
            hide(self.panel, cx);
        }
    }

    /// `panel` に `page` をはめた。
    fn track(&mut self, page: Page, panel: PanelId, cx: &mut Context<Self>) {
        self.shown = Some((page, panel));
        redraw(self.panel, cx);
    }
}

impl Render for Dashboard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let current = self.current(cx);
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
        let mirror = page_button(
            "dashboard-mirror",
            &icons::MONITOR,
            i18n::t(cx, "dashboard.mirror"),
            Page::Mirror,
        );
        let debug = page_button(
            "dashboard-debug",
            &icons::BUG,
            i18n::t(cx, "dashboard.debug"),
            Page::Debug,
        );
        let settings = page_button(
            "dashboard-settings",
            &icons::GEAR,
            i18n::t(cx, "dashboard.settings"),
            Page::Settings,
        );
        let dashboard = self.panel;
        components::card_panel(&theme)
            .rounded(px(design::radius::FULL))
            .flex()
            .items_center()
            .gap(px(spacing::SM))
            .px(px(spacing::SM))
            .child(
                components::button(
                    &theme,
                    "dashboard-quick-settings",
                    chrono::Local::now().format("%H:%M").to_string(),
                    variant(Page::QuickSettings),
                )
                .gap(px(spacing::SM))
                .tooltip(i18n::t(cx, "dashboard.quick_settings"))
                .children(battery.map(|b| design::icon(battery_glyph(&b)).size(px(font_size::LG))))
                .on_click(cx.listener(|this, _, _, cx| this.toggle(Page::QuickSettings, cx))),
            )
            .child(mirror)
            .child(debug)
            .child(settings)
            .child(div().flex_1())
            .child(
                components::icon_button(
                    &theme,
                    "dashboard-close",
                    &icons::X,
                    i18n::t(cx, "common.close"),
                    Variant::Default,
                )
                .on_click(move |_, _, cx| hide(dashboard, cx)),
            )
    }
}

/// クイック設定・画面のミラーリングの起動仕様（`page` はそのどちらか）。開くと `opened` でダッシュボードにはまる。
/// 閉じるのはナビゲーションバーかバーの同じボタン。
fn screen_spec(
    page: Page,
    dashboard: WeakEntity<Dashboard>,
    opened: Box<dyn FnOnce(PanelId, &mut App)>,
) -> PanelSpec {
    let title = match page {
        Page::Mirror => "dashboard.mirror",
        _ => "dashboard.quick_settings",
    };
    PanelSpec::new(Pose::IDENTITY, move |id, _, cx: &mut Context<Screen>| {
        opened(id, cx);
        Screen::new(id, page, dashboard, cx)
    })
    .sized(SIZE_M, RES_PX)
    .titled(title)
}

/// ダッシュボードから開く画面のうち、ダッシュボード自身のもの（クイック設定・画面のミラーリング）。
pub struct Screen {
    panel: PanelId,
    pub page: Page,
    dashboard: WeakEntity<Dashboard>,
    /// ミラーリングの取得元と、そのアイコンの画像（ウィンドウだけ）。
    sources: Vec<(Source, Option<Arc<RenderImage>>)>,
    pending: Option<Sources>,
    error: Option<String>,
    scroll: ScrollHandle,
}

impl Screen {
    fn new(
        panel: PanelId,
        page: Page,
        dashboard: WeakEntity<Dashboard>,
        cx: &mut Context<Self>,
    ) -> Self {
        tick_every_second(panel, cx);
        let mut this = Self {
            panel,
            page,
            dashboard,
            sources: Vec::new(),
            pending: None,
            error: None,
            scroll: ScrollHandle::new(),
        };
        if page == Page::Mirror {
            this.refresh();
        }
        this
    }

    /// 取得元を取り直す（取得は別スレッド。届くまでは 1 秒ごとの描き直しで見に来る）。
    fn refresh(&mut self) {
        match Sources::refresh() {
            Ok(pending) => (self.pending, self.error) = (Some(pending), None),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// 取得元の一覧が届いていれば受け取る。
    fn poll_sources(&mut self) {
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

    fn mirror(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        self.poll_sources();
        let dashboard = self.dashboard.clone();
        let tiles = |kind: SourceKind| {
            let tiles: Vec<_> = self
                .sources
                .iter()
                .enumerate()
                .filter(|(_, (s, _))| s.kind == kind)
                .map(|(i, (source, image))| {
                    let glyph = match (kind, image) {
                        (_, Some(image)) => img(image.clone())
                            .size(px(TILE_ICON_PX))
                            .into_any_element(),
                        (SourceKind::Screen, None) => tile_icon(&icons::MONITOR),
                        (SourceKind::Window, None) => tile_icon(&icons::APP_WINDOW),
                    };
                    let (source, dashboard) = (source.clone(), dashboard.clone());
                    components::tile(theme, format!("dashboard-source-{i}"))
                        .h(px(TILE_PX))
                        .min_w_0()
                        .child(glyph)
                        .child(tile_label(source.name.clone()))
                        .on_click(move |_, _, cx| {
                            // 映したものは、この一覧と入れ替わりにダッシュボードにはまる。
                            cx.default_global::<PanelControl>()
                                .spawns
                                .push(mirror_spec(source.clone(), dashboard.clone()));
                        })
                        .into_any_element()
                })
                .collect();
            if tiles.is_empty() {
                let note = if self.pending.is_some() {
                    i18n::t(cx, "dashboard.loading")
                } else {
                    i18n::t(cx, "dashboard.none")
                };
                div()
                    .px(px(spacing::SM))
                    .text_color(rgba(theme.neutral_text_low))
                    .child(note)
            } else {
                grid().children(tiles)
            }
        };
        let screens = tiles(SourceKind::Screen);
        let windows = tiles(SourceKind::Window);
        components::panel(theme).child(
            div()
                .id("dashboard-sources")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .flex()
                .flex_col()
                .gap(px(spacing::SM))
                .child(
                    section(theme, i18n::t(cx, "dashboard.screens")).child(
                        components::icon_button(
                            theme,
                            "dashboard-refresh",
                            &icons::ARROW_CLOCKWISE,
                            i18n::t(cx, "dashboard.refresh"),
                            Variant::OnBackground,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh();
                            redraw(this.panel, cx);
                        })),
                    ),
                )
                .when_some(self.error.clone(), |d, error| {
                    d.child(
                        div()
                            .px(px(spacing::SM))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::tf(cx, "dashboard.failed", &[("error", &error)])),
                    )
                })
                .child(screens)
                .child(section(theme, i18n::t(cx, "dashboard.windows")))
                .child(windows),
        )
    }

    fn quick_settings(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let now = chrono::Local::now();
        let ctl = cx.global::<PanelControl>();
        let mut batteries = ctl.batteries.clone();
        let mode = ctl.settings.theme;
        // HMD・左・右・トラッカーの順に並べる（ランタイムが返す順は決まらない）。
        batteries.sort_by_key(|b| match &b.device {
            Device::Hmd => (0, String::new()),
            Device::Controller(Hand::Left) => (1, String::new()),
            Device::Controller(Hand::Right) => (2, String::new()),
            Device::Tracker(id) => (3, id.clone()),
        });
        let mut trackers = 0;
        let cards: Vec<_> = batteries
            .iter()
            .map(|b| {
                let (id, name) = match &b.device {
                    Device::Hmd => ("battery-hmd".into(), i18n::t(cx, "dashboard.hmd")),
                    Device::Controller(Hand::Left) => (
                        "battery-left".into(),
                        i18n::t(cx, "dashboard.left_controller"),
                    ),
                    Device::Controller(Hand::Right) => (
                        "battery-right".into(),
                        i18n::t(cx, "dashboard.right_controller"),
                    ),
                    Device::Tracker(_) => {
                        trackers += 1;
                        (
                            format!("battery-tracker-{trackers}"),
                            i18n::tf(cx, "dashboard.tracker", &[("n", &trackers)]),
                        )
                    }
                };
                battery_card(theme, id, b, name, cx)
            })
            .collect();
        let dashboard = self.dashboard.clone();
        let theme_label = match mode {
            kernel::ThemeMode::Dark => i18n::t(cx, "theme.dark"),
            kernel::ThemeMode::Light => i18n::t(cx, "theme.light"),
        };
        // トラッカーが多いと収まらないので、中身は縦にスクロールする。
        components::panel(theme).child(
            div()
                .id("quick-settings")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(spacing::MD))
                // Android のクイック設定のように、上に時刻・日付と設定へのボタン。
                .child(
                    div()
                        .flex()
                        .items_center()
                        .px(px(spacing::SM))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .items_baseline()
                                .gap(px(spacing::MD))
                                .child(
                                    div()
                                        .debug_selector(|| "quick-time".into())
                                        .text_size(px(QUICK_TIME_PX))
                                        .font_weight(FontWeight::BOLD)
                                        .child(now.format("%H:%M").to_string()),
                                )
                                .child(
                                    div()
                                        .text_size(px(font_size::MD))
                                        .text_color(rgba(theme.neutral_text_low))
                                        .child(date(now, cx)),
                                ),
                        )
                        .child(
                            components::icon_button(
                                theme,
                                "quick-settings-open",
                                &icons::GEAR,
                                i18n::t(cx, "dashboard.settings"),
                                Variant::OnBackground,
                            )
                            // バーの設定ボタンと同じ（設定画面がこの画面と入れ替わりにはまる）。
                            .on_click(move |_, _, cx| {
                                let _ = dashboard.update(cx, |d, cx| d.toggle(Page::Settings, cx));
                            }),
                        ),
                )
                .child(
                    grid().child(
                        components::tile(theme, "quick-theme")
                            .h(px(TILE_PX))
                            .min_w_0()
                            .child(tile_icon(match mode {
                                kernel::ThemeMode::Dark => &icons::MOON,
                                kernel::ThemeMode::Light => &icons::SUN,
                            }))
                            .child(tile_label(format!(
                                "{} · {theme_label}",
                                i18n::t(cx, "dashboard.theme")
                            )))
                            .on_click(move |_, _, cx| {
                                cx.default_global::<PanelControl>()
                                    .setting_changes
                                    .push(Setting::Theme(mode.toggled()))
                            }),
                    ),
                )
                .child(section(theme, i18n::t(cx, "dashboard.batteries")))
                .child(if cards.is_empty() {
                    div()
                        .px(px(spacing::SM))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(i18n::t(cx, "dashboard.no_batteries"))
                } else {
                    grid().children(cards)
                }),
        )
    }
}

impl Render for Screen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        match self.page {
            Page::Mirror => self.mirror(&theme, cx),
            // デバッグ・設定は main が渡す別の画面で、Screen にはならない。
            _ => self.quick_settings(&theme, cx),
        }
    }
}

/// 配信を映すパネル。画像はカーネルが直接送るので、描くのは始まるまで・終わった後の状態だけ。
struct Mirror {
    panel: PanelId,
}

impl Render for Mirror {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let status = cx
            .try_global::<PanelControl>()
            .and_then(|c| c.panels.get(&self.panel))
            .and_then(|s| s.status.desktop.clone());
        let text = match status {
            None | Some(Status::Starting) => i18n::t(cx, "desktop.starting"),
            Some(Status::Streaming) => i18n::t(cx, "desktop.streaming"),
            Some(Status::Closed) => i18n::t(cx, "desktop.closed"),
            Some(Status::Failed(e)) => i18n::tf(cx, "desktop.failed", &[("error", &e)]),
        };
        components::card_panel(&theme)
            .flex()
            .items_center()
            .justify_center()
            .p(px(spacing::XL))
            .text_color(rgba(theme.neutral_text_low))
            .child(text)
    }
}

/// `source` を映すパネルの起動仕様。開くとダッシュボードにはまる（ミラーリングの一覧と入れ替わる）。
/// タイトルは取得元の名前で、ナビゲーションバーで閉じると配信も止まる。
fn mirror_spec(source: Source, dashboard: WeakEntity<Dashboard>) -> PanelSpec {
    let title = source.name.clone();
    PanelSpec::new(Pose::IDENTITY, move |panel, _, cx| {
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .commands
            .push(PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE));
        ui::dock(panel, cx);
        let _ = dashboard.update(cx, |d, cx| d.track(Page::Mirror, panel, cx));
        Mirror { panel }
    })
    // 配信は横幅を保って縦横比を合わせるので、幅をはめる画面とそろえる。
    .sized(SIZE_M, RES_PX)
    .in_view(SCREEN_POSE)
    .titled_text(title)
    .dockable()
    // 映しているデスクトップ・ウィンドウへ打てるよう、ナビゲーションバーの左端に OS キーボードのトグルを出す。
    .with_os_keyboard()
}

/// 電池 1 つのカード: デバイスのアイコンと名前、残量（数字と棒）。充電中はアイコンの形と文字でも示す。
fn battery_card(
    theme: &Theme,
    id: String,
    b: &Battery,
    name: gpui::SharedString,
    cx: &App,
) -> gpui::AnyElement {
    let low = b.level < LOW_BATTERY && !b.charging;
    let fill = if low {
        theme.danger_solid
    } else {
        theme.accent_solid
    };
    let device = match b.device {
        Device::Hmd => &icons::VIRTUAL_REALITY,
        Device::Controller(_) => &icons::GAME_CONTROLLER,
        Device::Tracker(_) => &icons::CROSSHAIR,
    };
    components::card(theme)
        .debug_selector(move || id)
        .h(px(TILE_PX))
        .min_w_0()
        .p(px(spacing::MD))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .child(design::icon(device).size(px(font_size::LG)))
                .child(tile_label(name).text_left()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .child(design::icon(battery_glyph(b)).size(px(font_size::LG)))
                .child(
                    div()
                        .text_size(px(font_size::LG))
                        .font_weight(FontWeight::BOLD)
                        .child(percent(b.level)),
                )
                .when(b.charging, |d| {
                    d.child(
                        div()
                            .text_size(px(font_size::SM))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(i18n::t(cx, "dashboard.charging")),
                    )
                }),
        )
        .child(
            div()
                .h(px(spacing::SM))
                .rounded(px(design::radius::FULL))
                .bg(rgba(theme.groove))
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(b.level))
                        .rounded(px(design::radius::FULL))
                        .bg(rgba(fill)),
                ),
        )
        .into_any_element()
}

/// 残量に合った電池のアイコン（充電中は稲妻）。
fn battery_glyph(b: &Battery) -> &'static design::Icon {
    match b.level {
        _ if b.charging => &icons::BATTERY_CHARGING,
        l if l >= 0.9 => &icons::BATTERY_FULL,
        l if l >= 0.6 => &icons::BATTERY_HIGH,
        l if l >= 0.35 => &icons::BATTERY_MEDIUM,
        l if l >= 0.1 => &icons::BATTERY_LOW,
        _ => &icons::BATTERY_EMPTY,
    }
}

fn percent(level: f32) -> String {
    format!("{}%", (level * 100.0).round() as u32)
}

/// 今の言語での日付（書式は文言の `common.date`。chrono の書式で、曜日だけを `{weekday}` で訳す）。
pub fn date(now: chrono::DateTime<chrono::Local>, cx: &App) -> String {
    use chrono::Datelike;
    let weekday = match now.weekday() {
        chrono::Weekday::Sun => i18n::t(cx, "weekday.sun"),
        chrono::Weekday::Mon => i18n::t(cx, "weekday.mon"),
        chrono::Weekday::Tue => i18n::t(cx, "weekday.tue"),
        chrono::Weekday::Wed => i18n::t(cx, "weekday.wed"),
        chrono::Weekday::Thu => i18n::t(cx, "weekday.thu"),
        chrono::Weekday::Fri => i18n::t(cx, "weekday.fri"),
        chrono::Weekday::Sat => i18n::t(cx, "weekday.sat"),
    };
    let format = i18n::tf(cx, "common.date", &[("weekday", &weekday)]);
    now.format(&format).to_string()
}

/// 見出しの行（右に操作を足せる）。高さはボタンと同じにして、操作の有無で一覧の位置が変わらないようにする。
fn section(theme: &Theme, title: gpui::SharedString) -> Div {
    div()
        .h(px(components::NAV_BAR_HEIGHT - 2.0 * spacing::SM))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(spacing::SM))
        .text_size(px(font_size::MD))
        .font_weight(FontWeight::BOLD)
        .text_color(rgba(theme.neutral_text_low))
        .child(title)
}

fn grid() -> Div {
    div().grid().grid_cols(COLUMNS).gap(px(spacing::SM))
}

fn tile_icon(glyph: &'static design::Icon) -> gpui::AnyElement {
    design::icon(glyph)
        .size(px(TILE_ICON_PX))
        .into_any_element()
}

/// タイルの文字（1 行。長ければ省略）。
fn tile_label(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .text_center()
        .truncate()
        .child(text.into())
}

/// アイコンの RGBA を GPUI の画像（BGRA）にする。
fn render_image(icon: &Icon) -> Option<Arc<RenderImage>> {
    let mut bgra = icon.rgba.to_vec();
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(icon.width, icon.height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// 1 秒ごとに描き直す（時計・電池・取得元の一覧の到着）。ビューが無くなれば止まる。
fn tick_every_second<V: 'static>(panel: PanelId, cx: &mut Context<V>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| redraw(panel, cx)).is_err() {
                return;
            }
        }
    })
    .detach();
}

/// 描き直しを頼む（フレームループはビューの notify を知らないので `redraw` で頼む）。
fn redraw<V: 'static>(panel: PanelId, cx: &mut Context<V>) {
    cx.notify();
    cx.default_global::<PanelControl>()
        .panels
        .entry(panel)
        .or_default()
        .redraw = true;
}

fn close(panel: PanelId, cx: &mut App) {
    commands(panel, [PanelCommand::Close], cx);
}

fn commands(panel: PanelId, commands: impl IntoIterator<Item = PanelCommand>, cx: &mut App) {
    cx.default_global::<PanelControl>()
        .panels
        .entry(panel)
        .or_default()
        .commands
        .extend(commands);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};

    fn battery(level: f32, charging: bool) -> Battery {
        Battery {
            device: Device::Hmd,
            level,
            charging,
        }
    }

    #[test]
    fn helpers_format_and_pick_glyphs() {
        assert!(std::ptr::eq(battery_glyph(&battery(1.0, false)), &icons::BATTERY_FULL));
        assert!(std::ptr::eq(battery_glyph(&battery(0.5, false)), &icons::BATTERY_MEDIUM));
        assert!(std::ptr::eq(battery_glyph(&battery(0.05, false)), &icons::BATTERY_EMPTY));
        assert!(std::ptr::eq(battery_glyph(&battery(0.05, true)), &icons::BATTERY_CHARGING));
        assert_eq!(percent(0.826), "83%");
    }

    /// バーを `pose` に置くと、はめた画面は目の高さの正面に垂直に立ち、バーはその下で 1m 以上離れる。
    #[test]
    fn docked_screen_stands_upright_at_eye_level_in_front() {
        use kernel::overlay::{Overlay, Parent};
        use kernel::subsystems::Subsystems;
        use kernel::vr::mock::MockBackend;
        let mut vr = Subsystems::with_backend(MockBackend::default());
        let mut ov = Overlay::new();
        let bar = ov
            .add_panel(&mut vr.backend, "bar", pose(), BAR_M, BAR_PX)
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
        assert!(s.position.abs_diff_eq(SCREEN_POSE.position, 1e-3), "{}", s.position);
        assert!(s.orientation.abs_diff_eq(Quat::IDENTITY, 1e-4));
        let b = ov.panel(bar).pose.position;
        assert!(b.y < -0.3 && b.length() >= 1.0, "{b}");
    }

    fn summon(opened: Box<dyn FnOnce(PanelId, &mut App)>) -> PanelSpec {
        PanelSpec::new(Pose::IDENTITY, move |id, _, cx| {
            opened(id, cx);
            Mirror { panel: id }
        })
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let at = cx.debug_bounds(selector).unwrap().center();
        cx.simulate_click(at, Modifiers::none());
    }

    /// 要求された一番新しいパネルを `id` で開く。
    fn open_spawned(cx: &mut VisualTestContext, id: PanelId) {
        cx.update(|window, cx| {
            let spec = cx.default_global::<PanelControl>().spawns.pop().unwrap();
            let _ = spec.build(id, window, cx);
        });
    }

    fn commands(cx: &mut VisualTestContext, id: PanelId) -> Vec<PanelCommand> {
        cx.update(|_, cx| {
            cx.global::<PanelControl>()
                .panels
                .get(&id)
                .map_or(Vec::new(), |p| p.commands.clone())
        })
    }

    fn docked_to_bar(c: &PanelCommand) -> bool {
        matches!(
            c,
            PanelCommand::Parent(Some(kernel::Parent {
                panel: 1,
                docked: true,
                slot: Some(_),
                ..
            }))
        )
    }

    #[gpui::test]
    fn opening_shows_nothing_and_buttons_toggle_docked_screens(cx: &mut TestAppContext) {
        // バー（PanelId 1）の上を、画面をはめる場所にする。
        let (bar, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        });
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()), 0);
        // 設定を押すと、main が渡した設定画面がバーの上にはまる。
        click(cx, "dashboard-settings");
        open_spawned(cx, 2);
        assert!(commands(cx, 2).iter().any(docked_to_bar));
        assert_eq!(cx.update(|_, cx| bar.read(cx).current(cx)), Some(Page::Settings));
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
        // X は閉じずに隠す。
        click(cx, "dashboard-close");
        assert!(commands(cx, 1).contains(&PanelCommand::Hide(true)));
        assert!(!commands(cx, 1).contains(&PanelCommand::Close));
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

    #[gpui::test]
    fn quick_settings_lists_batteries_in_device_order(cx: &mut TestAppContext) {
        let (_screen, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().batteries = vec![
                Battery {
                    device: Device::Tracker("LHR-2".into()),
                    level: 0.4,
                    charging: false,
                },
                Battery {
                    device: Device::Controller(Hand::Right),
                    level: 0.7,
                    charging: true,
                },
                battery(0.9, false),
            ];
            Screen::new(2, Page::QuickSettings, WeakEntity::new_invalid(), cx)
        });
        cx.run_until_parked();
        // HMD・右コントローラ・トラッカーの順で左から並ぶ。
        let x = ["battery-hmd", "battery-right", "battery-tracker-1"]
            .map(|s| cx.debug_bounds(s).unwrap().origin.x);
        assert!(x[0] < x[1] && x[1] < x[2]);
    }

    #[gpui::test]
    fn choosing_a_source_docks_its_mirror_without_closing_the_dashboard(
        cx: &mut TestAppContext,
    ) {
        let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
        let bar = cx
            .add_window(|_, cx| {
                cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
                Dashboard::new(1, summon, summon, cx)
            })
            .root(cx)
            .unwrap();
        // バーにはまったミラーリングの一覧（PanelId 2）。
        let (list, cx) = cx.add_window_view(|_, cx| {
            ui::dock(2, cx);
            let mut list = Screen::new(2, Page::Mirror, bar.downgrade(), cx);
            list.pending = None;
            list.sources = vec![(source.clone(), None)];
            list
        });
        let _ = list;
        cx.run_until_parked();
        click(cx, "dashboard-source-0");
        open_spawned(cx, 4);
        let c = commands(cx, 4);
        assert_eq!(c[0], PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE));
        assert!(docked_to_bar(&c[1]));
        // 一覧は入れ替わりに閉じ、ダッシュボードは開いたまま。バーのミラーリングのボタンが光る。
        assert!(commands(cx, 2).contains(&PanelCommand::Close));
        assert!(!commands(cx, 1).contains(&PanelCommand::Close));
        assert_eq!(cx.update(|_, cx| bar.read(cx).current(cx)), Some(Page::Mirror));
    }
}
