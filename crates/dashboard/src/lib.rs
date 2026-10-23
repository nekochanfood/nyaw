//! Layer 6 画面: ダッシュボード。左手の X ボタンを二回続けて押すと開き、もう一度二回押すと閉じる
//! （開け閉めは常駐の wrist が [`kernel::PanelStatus::double_press`] を見て行う）。開いている間は空間全体が暗くなる。
//! ダッシュボードは少し上を向けたバー（[`Dashboard`]）そのもので、開いた時は何も出さない。バーのボタンで、
//! クイック設定・ランチャー・再生中（[`Screen`]）・デバッグ・設定の画面（それぞれ自分のナビゲーションバーを持つ別のパネル）を
//! バーの上にはめる（`ui::dock`。同じボタンをもう一度押すと閉じる）。ランチャーで開いたアプリのウィンドウもそこにはまる。
//! はめた画面はダッシュボードに固定され（一緒に動き、掴むとダッシュボードごと動く）、ナビゲーションバーから外して
//! 独立させたり、戻したりできる。閉じる（バーの X・X ボタンの二回押し）と、はめた画面ごと隠すだけで、次は同じ状態で出る。

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use design::components::slide::{Entering, Slide, fade_in, fit_bar, slide_in, with_leaving};
use design::components::{self, Variant};
use design::{Theme, font_size, icons, spacing};
use gpui::{
    AnyElement, App, Context, Div, FocusHandle, FontWeight, KeyDownEvent, ListAlignment, ListState,
    RenderImage, ScrollHandle, SharedString, Task, WeakEntity, Window, div, img, list, prelude::*,
    px, rgba,
};
use kernel::audio::Sound;
use kernel::desktop::{
    self, DEFAULT_FRAME_RATE, Icon, InstalledApp, InstalledApps, Source, SourceKind, Sources,
    Status,
};
use kernel::subsystems::media;
use kernel::vr::{Battery, Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId, Setting};
use ui::mirror::{MirrorState, mirror_screen};
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
const BAR_M: Vec2 = Vec2::new(SIZE_M.x, SIZE_M.x * BAR_PX.1 as f32 / BAR_PX.0 as f32);
/// 画面を開いてからはまるまでの、最初の置き場所（はまる所）。
const SCREEN_POSE: Pose = Pose::new(Vec3::new(0.0, 0.0, -DISTANCE), Quat::IDENTITY);
/// グリッドの列の数とタイルの高さ（px）。
const COLUMNS: u16 = 4;
const TILE_PX: f32 = 120.0;
/// タイルのアイコンの大きさ（px）。
const TILE_ICON_PX: f32 = 48.0;
/// 再生中の画面（背景に大きな画像が載る）の Ghost ボタンの、ホバー・押下の背景の不透明度。
const NOW_PLAYING_BACKGROUND_OPACITY: f32 = 0.125;
/// アプリ一覧の列の数と、起動中のアプリの印（点）の大きさ（px）。
const APP_COLUMNS: u16 = 6;
const RUNNING_DOT_PX: f32 = 6.0;
/// アプリを起動してから、ウィンドウが出るのを待つ長さ。出なければ待機中の画面に見つからないと出す（起動はしている）。
const LAUNCH_WAIT: Duration = Duration::from_secs(20);
/// 起動したアプリのウィンドウを探し直す間隔。
const LAUNCH_POLL: Duration = Duration::from_millis(500);
/// クイック設定の上の時刻の大きさ（px）。
const QUICK_TIME_PX: f32 = 48.0;
/// 再生中の画面: ジャケットの一辺、シークバーの幅・棒の太さ・押せる高さ（px）、背景に重ねる地の濃さ（0..=255）。
const ART_DISPLAY_PX: f32 = 280.0;
const SEEK_PX: f32 = 560.0;
const SEEK_BAR_PX: f32 = 6.0;
const SEEK_HIT_PX: f32 = 32.0;
const NOW_PLAYING_VEIL: u32 = 0x8c;
/// 進行中の作業の知らせ（ライブアクティビティ。丸いボタン）の、進み具合の輪の一辺と太さ・中のアイコンの一辺・
/// 長い文字（`時:分:秒`）の大きさ（px）。
const ACTIVITY_RING_PX: f32 = 48.0;
const ACTIVITY_RING_STROKE: f32 = 4.0;
const ACTIVITY_ICON_PX: f32 = 28.0;
const ACTIVITY_SMALL_TEXT_PX: f32 = 11.0;
/// これより少ない残量は危険の色にする。
const LOW_BATTERY: f32 = 0.2;

/// 開くとき・閉じるときの音。どちらもダッシュボード自身から聞こえる（閉じる音は閉じ方を問わず鳴る）。
pub const OPEN_SOUND: &[u8] = include_bytes!("../assets/dashboard_open.wav");
pub const CLOSE_SOUND: &[u8] = include_bytes!("../assets/dashboard_close.wav");

/// ダッシュボードから開くほかの画面（デバッグ・設定）の起動仕様を作る。第 1 引数は開いた元のパネル
/// （[`PanelSpec::spawn`] に渡す）。開いたら引数で PanelId を知らせる（[`spec`] と同じ形）。
/// 画面どうしを直接つながないよう、何を開くかは main が選ぶ。
pub type Summon = fn(PanelId, Opened) -> PanelSpec;
pub type Opened = Box<dyn FnOnce(PanelId, &mut App)>;

/// バーの置き場所（開いた瞬間の頭から見た相対姿勢）。はめた画面（[`SIZE_M`]）が目の高さの正面 [`DISTANCE`] に垂直に立つよう、
/// 画面の下半分・そのナビゲーションバー・隙間の分だけ下げ、傾けたバーの上端がその真下に来るように置く
/// （オーバーレイ層の `Parent::slot` の置き方の逆）。
pub fn pose() -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    let face = SIZE_M.y / 2.0 - margin(SIZE_M, RES_PX);
    // はめた画面のナビゲーションバーは、距離で大きさを変えない（1m 先での大きさのまま）。
    let nav_bar = ui::NAV_BAR_HEIGHT_M;
    let top = Vec3::new(
        0.0,
        -(face + nav_bar + kernel::overlay::SLOT_GAP),
        -DISTANCE,
    );
    let rotation = Quat::from_rotation_x(-BAR_TILT);
    let half = BAR_M.y / 2.0 - margin(BAR_M, BAR_PX);
    Pose::new(top - rotation * Vec3::Y * half, rotation)
}

/// ダッシュボード（バー）の起動仕様。`opened` は作られたパネルの ID を受け取る。`hidden` なら隠したまま開く
/// （起動時に作っておき、画面の配信を用意しておく。出すのは [`show`]）。
pub fn spec(
    opened: impl FnOnce(PanelId, &mut App) + 'static,
    settings: Summon,
    debug: Summon,
    hidden: bool,
) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, move |id, _, cx: &mut Context<Dashboard>| {
        nyapp::opened(nyapp::system::DASHBOARD.id, id, cx);
        opened(id, cx);
        let ctl = cx.default_global::<PanelControl>();
        ctl.panels.entry(id).or_default().commands.extend([
            if hidden {
                PanelCommand::Hide(true)
            } else {
                PanelCommand::Play(Sound::Wav(OPEN_SOUND), false)
            },
            PanelCommand::CloseSound(Sound::Wav(CLOSE_SOUND)),
        ]);
        // バーの上を、画面をはめる場所にする。
        ctl.dock = Some((id, BAR_TILT));
        Dashboard::new(id, settings, debug, cx)
    })
    .sized(BAR_M, BAR_PX)
    .in_view(pose())
    // 閉じる（隠す）のは X ボタンの二回押しかダッシュボードの外を押したときで、閉じるボタンは置かない。
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

/// 隠したダッシュボードを、今の目の前に同じ状態で出し直す。はまっているのが配信なら、そのウィンドウを OS で前に出す。
pub fn show(dashboard: PanelId, cx: &mut App) {
    if let Some(docked) = ui::docked(cx) {
        commands(docked, [PanelCommand::FocusSource], cx);
    }
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
    /// 再生中のメディア（[`Screen`]）。
    NowPlaying,
    /// ランチャー: アプリを探して起動し、そのウィンドウを映す（[`Screen`]）。
    Launcher,
    Debug,
    Settings,
}

/// ダッシュボード（バー）。左からメニュー（デバッグ・設定のポップオーバー）・時刻と電池（クイック設定）・アプリを
/// 開くランチャー、区切り線を挟んで開いた配信のボタン（溜まっていき、はみ出すと横にスクロールする）。
/// 閉じる（隠す）のは X ボタンの二回押しか、ダッシュボードの外を押したとき。
/// 今はまっている画面・配信のボタンは強調色。
pub struct Dashboard {
    panel: PanelId,
    settings: Summon,
    debug: Summon,
    /// ダッシュボードで開いた配信（画面・ウィンドウ）。押すとはまり、ほかを選ぶと閉じずに隠れる。
    streams: Vec<Stream>,
    /// 開いているメニューのポップオーバー（[`Menu`]）。
    menu: Option<PanelId>,
    /// 最後にはめた画面（外されたり閉じられたりしていれば、[`Self::current`] は `None`）。
    shown: Option<(Page, PanelId)>,
    /// 反映した、何も無い所を押した回数（[`kernel::PanelStatus::outside_press`]）と、そのとき隠していたか。
    /// 最初に見た値から数える。
    outside: Option<(u32, bool)>,
    /// 足されたボタンの出方（[`Entering`]）。
    enter: Entering,
    /// 前のフレームに並べていたもの（消えていくものを含む）。消えたものを、滑り出る間だけ同じ場所に描き残すのに使う。
    prev_streams: Vec<Stream>,
    prev_activities: Vec<nyapp::activity::Live>,
    prev_media: bool,
    prev_sep: bool,
    /// バーの横スクロール（最大幅を超えたとき）。
    scroll: ScrollHandle,
}

impl Dashboard {
    fn new(panel: PanelId, settings: Summon, debug: Summon, cx: &mut Context<Self>) -> Self {
        tick_every_second(panel, cx);
        cx.observe_global::<PanelControl>(Self::close_on_outside_press)
            .detach();
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
        if let Ok(pending) = Sources::refresh() {
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
            prev_streams: Vec::new(),
            prev_activities: Vec::new(),
            prev_media: false,
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
            .offset(menu_offset())
            .without_resize();
            cx.default_global::<PanelControl>().spawns.push(spec);
        }
        redraw(self.panel, cx);
    }

    /// 配信 `panel` を開いた（はめる・隠すのは開く側。`resident` なら常駐のもので、隠して開いた）。バーにボタンが増える。
    fn add_stream(
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
            resident,
            seen_docked: false,
        });
        redraw(self.panel, cx);
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
    fn minimize(&mut self, id: PanelId, cx: &mut Context<Self>) {
        self.stash(id, cx);
        redraw(self.panel, cx);
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
        let docked = ui::docked(cx) == Some(id);
        self.stash_docked(cx);
        if !docked && let Some(stream) = self.streams.iter_mut().find(|s| s.panel == id) {
            // はまったと分かるまでは、外れたとみなして消さない。
            (stream.stashed, stream.seen_docked) = (false, false);
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
        redraw(self.panel, cx);
    }

    /// 配信 `id` がダッシュボードの配信で、はまっていなければはめる（独立させたものは動かさない）。
    fn show_stream(&mut self, id: PanelId, cx: &mut Context<Self>) {
        if ui::docked(cx) != Some(id) && self.streams.iter().any(|s| s.panel == id) {
            self.toggle_stream(id, cx);
        }
    }

    /// 閉じた・自分で外して独立させた配信のボタンを消す。隠しているもの（[`Stream::stashed`]）と常駐のものは残す。
    fn prune_streams(&mut self, cx: &App) {
        let (ctl, bar) = (cx.global::<PanelControl>(), self.panel);
        self.streams.retain_mut(|s| {
            let status = ctl.panels.get(&s.panel).map(|p| &p.status);
            if status.is_some_and(|st| st.closed) {
                return false;
            }
            let docked = status.and_then(|st| st.parent) == Some(bar);
            s.seen_docked |= docked;
            s.resident || s.stashed || !s.seen_docked || docked
        });
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
        redraw(self.panel, cx);
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
        redraw(self.panel, cx);
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
        redraw(self.panel, cx);
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
struct Stream {
    panel: PanelId,
    kind: SourceKind,
    name: SharedString,
    /// ウィンドウのアプリのアイコン（無ければウィンドウの記号）。
    icon: Option<Arc<RenderImage>>,
    /// ほかを選んだので隠して外している。
    stashed: bool,
    /// 常駐（起動時に用意した画面）。閉じずにしまい、独立させてもバーに残る。
    resident: bool,
    /// ダッシュボードにはまっているのを一度でも見た（その後に外れたら、自分で外して独立させたもの）。
    seen_docked: bool,
}

/// メニューのポップオーバーの大きさと解像度（2 行の一覧が収まる）。
const MENU_PX: (u32, u32) = (
    320,
    (MENU_ROWS * (components::NAV_BAR_HEIGHT - 2.0 * spacing::SM)
        + (MENU_ROWS - 1.0)
        + 2.0 * components::PANEL_MARGIN) as u32,
);
/// メニューの行の数（デバッグ・設定・終了）。
const MENU_ROWS: f32 = 3.0;
/// 終了の確かめのダイアログの幅（px。1px = 1mm）と、視界の中での置き場所。
const QUIT_DIALOG_WIDTH: u32 = 440;
const QUIT_DIALOG_POSE: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
const MENU_M: Vec2 = Vec2::new(
    MENU_PX.0 as f32 * SIZE_M.x / RES_PX.0 as f32,
    MENU_PX.1 as f32 * SIZE_M.x / RES_PX.0 as f32,
);
/// メニューを、はまっている画面よりどれだけ手前に出すか（m）。
const MENU_LIFT: f32 = 0.03;

/// メニューのポップオーバーの、バーから見た置き場所: バーの左端の上に、はめた画面と同じく垂直に立て、画面より少し手前に出す。
fn menu_offset() -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    let rotation = Quat::from_rotation_x(BAR_TILT);
    let top = Vec3::new(
        (MENU_M.x - BAR_M.x) / 2.0,
        BAR_M.y / 2.0 - margin(BAR_M, BAR_PX),
        0.0,
    );
    let lift = kernel::overlay::SLOT_GAP + MENU_M.y / 2.0 - margin(MENU_M, MENU_PX);
    Pose::new(top + rotation * Vec3::new(0.0, lift, MENU_LIFT), rotation)
}

/// メニューのポップオーバー: デバッグ・設定（選ぶとその画面をはめる）と、一番下にアプリの終了（確かめのダイアログを出す）。
/// 選ぶと閉じる。背景は敷かず、一覧のカードがそのままポップオーバーになる。
pub struct Menu {
    panel: PanelId,
    dashboard: WeakEntity<Dashboard>,
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let item = |id, glyph, key, page| {
            let (dashboard, panel) = (self.dashboard.clone(), self.panel);
            components::menu_item(&theme, id, glyph, i18n::t(cx, key)).on_click(move |_, _, cx| {
                close(panel, cx);
                let _ = dashboard.update(cx, |d, cx| d.toggle(page, cx));
            })
        };
        let panel = self.panel;
        let quit = components::menu_item(
            &theme,
            "menu-quit",
            &icons::POWER,
            i18n::t(cx, "dashboard.quit"),
        )
        .on_click(move |_, window, cx| {
            close(panel, cx);
            let spec = quit_dialog_spec(panel, window, cx);
            cx.default_global::<PanelControl>().spawns.push(spec);
        });
        components::card_panel(&theme)
            .rounded(px(design::radius::LG))
            .child(components::list(
                &theme,
                [
                    item("menu-debug", &icons::BUG, "dashboard.debug", Page::Debug),
                    item(
                        "menu-settings",
                        &icons::GEAR,
                        "dashboard.settings",
                        Page::Settings,
                    ),
                    quit,
                ],
            ))
    }
}

/// アプリを終了するかを確かめるダイアログの起動仕様。視界の中央に出す。
fn quit_dialog_spec(from: PanelId, window: &Window, cx: &App) -> PanelSpec {
    let height = components::dialog_height(
        window,
        QUIT_DIALOG_WIDTH,
        &i18n::t(cx, "dashboard.quit_title"),
        &i18n::t(cx, "dashboard.quit_body"),
        2,
    );
    PanelSpec::spawn(from, |panel, _, _| QuitDialog { panel })
        .sized(
            Vec2::new(QUIT_DIALOG_WIDTH as f32 / 1000.0, height as f32 / 1000.0),
            (QUIT_DIALOG_WIDTH, height),
        )
        // 開いた元のメニューはすぐ閉じるので、その手前ではなく視界の中央に出す。
        .in_view(QUIT_DIALOG_POSE)
        // ダイアログは答えて閉じるもの。ナビゲーションバーは付けない。
        .without_nav_bar()
}

/// アプリを終了するかを確かめるダイアログ。終了を選ぶとアプリを終える。
struct QuitDialog {
    panel: PanelId,
}

impl Render for QuitDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let panel = self.panel;
        components::dialog(
            &theme,
            "quit-dialog",
            &icons::POWER,
            i18n::t(cx, "dashboard.quit_title"),
            i18n::t(cx, "dashboard.quit_body"),
            [
                components::button(
                    &theme,
                    "quit-confirm",
                    i18n::t(cx, "dashboard.quit_confirm"),
                    Variant::Danger,
                )
                .on_click(|_, _, cx| cx.default_global::<PanelControl>().quit = true),
                components::button(
                    &theme,
                    "quit-cancel",
                    i18n::t(cx, "common.cancel"),
                    Variant::Default,
                )
                .on_click(move |_, _, cx| close(panel, cx)),
            ],
        )
        // 電源のアイコンは塗りだと形が潰れて分かりにくいので、線で描く。
        .weight(design::Weight::Regular)
    }
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
        let (mut enter, now_at) = (std::mem::take(&mut self.enter), Instant::now());
        let hidden = cx
            .global::<PanelControl>()
            .panels
            .get(&self.panel)
            .is_some_and(|p| p.status.hidden);
        enter.begin(hidden, now_at);
        // 左から数えたボタンの順番: メニュー 0・クイック設定 1・進行中の作業・再生中・ランチャー・区切り線と配信。
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
        let stream_slot = 3 + n_activity + media;
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
        if let Some(ix) = self.enter.take_fresh() {
            self.scroll.scroll_to_item(ix);
        }
        if self.enter.finish() {
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
        // ランチャー・区切り線と、開いた配信。
        let mut items: Vec<AnyElement> = vec![menu.into_any_element(), quick.into_any_element()];
        items.extend(activities.into_iter().map(IntoElement::into_any_element));
        if self.prev_media {
            items.push(now_playing.into_any_element());
        }
        items.push(mirror.into_any_element());
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

/// クイック設定・ランチャー・再生中の起動仕様（`page` はそのどれか）。開くと `opened` でダッシュボードにはまる。
/// 閉じるのはナビゲーションバーかバーの同じボタン。
fn screen_spec(
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

/// ダッシュボードから開く画面のうち、ダッシュボード自身のもの（クイック設定・ランチャー・再生中）。
pub struct Screen {
    panel: PanelId,
    pub page: Page,
    dashboard: WeakEntity<Dashboard>,
    /// ランチャーで映す取得元と、そのアイコンの画像（ウィンドウだけ）。
    sources: Vec<(Source, Option<Arc<RenderImage>>)>,
    pending: Option<Sources>,
    error: Option<String>,
    /// ランチャーの中身（見出し・タイルの行を並べた仮想化したリスト。見えている行だけを組み立てる）と、その行。
    list: ListState,
    launcher: Launcher,
    /// 再生中の画面の画像（ジャケット・背景・アプリのアイコン）。元の画像が変わったときだけ作り直す。
    media_images: Option<MediaImages>,
    /// 取得元の一覧が届いたら、再生中のアプリのウィンドウを開く。
    open_app: bool,
    /// アプリ一覧（名前の順）と、そのアイコンの画像。前に読んだもの（[`AppCatalog`]）を出しておき、取り直したら替える。
    apps: Vec<(InstalledApp, Option<Arc<RenderImage>>)>,
    /// アプリ一覧を取り直している最中か。
    apps_pending: bool,
    /// アプリ一覧の検索欄に打った文字。
    query: String,
    search: FocusHandle,
    /// 入っているニャプリ（名前の順）と、読み直している最中か。
    nyapps: Vec<nyapp::Installed>,
    nyapps_pending: bool,
    /// ニャプリを消す状態（タイルを押すと消す）。
    removing: bool,
    /// 更新の確認の結果（確認中も）。
    update_note: Option<SharedString>,
}

/// 読んだアプリ一覧。次に開いたときに、取り直すまでの間それを出す（アイコンを読むので時間が掛かる）。
struct AppCatalog(AppList);

/// アプリ一覧（名前の順）と、そのアイコンの画像。
type AppList = Vec<(InstalledApp, Option<Arc<RenderImage>>)>;

impl gpui::Global for AppCatalog {}

/// アプリ一覧を OS から取り直し、アイコンを画像にする（どちらも別スレッド。取得はアイコンを読むので数秒かかる）。
fn fetch_apps(cx: &App) -> Task<Result<AppList, String>> {
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
struct Launcher {
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

/// 再生中の画面の画像。`key` は元の画像（[`kernel::subsystems::media::Image`]）の中身の場所。
struct MediaImages {
    key: [usize; 3],
    art: Option<Arc<RenderImage>>,
    backdrop: Option<Arc<RenderImage>>,
    icon: Option<Arc<RenderImage>>,
}

impl Screen {
    fn new(
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
                        redraw(panel, cx);
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
                    redraw(s.panel, cx);
                });
            })
            .detach();
            this.scan_nyapps(cx);
        }
        this
    }

    /// 入っているニャプリを読み直す（パッケージを読むので別スレッド）。
    fn scan_nyapps(&mut self, cx: &mut Context<Self>) {
        self.nyapps_pending = true;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async { nyapp::installed::scan() })
                .await;
            let _ = this.update(cx, |s, cx| {
                (s.nyapps, s.nyapps_pending) = (found, false);
                redraw(s.panel, cx);
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
                            redraw(this.panel, cx);
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
                            redraw(this.panel, cx);
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
            app.icon(lang.code(), TILE_ICON_PX, theme)
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
            redraw(this.panel, cx);
        }))
        .into_any_element()
    }

    /// 再生中のメディアのアプリのウィンドウ（取得元の一覧から。実行ファイル名か、ウィンドウの名前で探す）。
    fn app_window(&self, app: &media::App) -> Option<&(Source, Option<Arc<RenderImage>>)> {
        let stem = |s: &str| {
            let s = s.to_ascii_lowercase();
            s.strip_suffix(".exe").map(str::to_owned).unwrap_or(s)
        };
        let (id, name) = (stem(&app.id), app.name.to_lowercase());
        let exe = app.exe.as_deref().map(stem);
        self.sources.iter().find(|(s, _)| {
            s.kind == SourceKind::Window
                && (s.app.as_deref().is_some_and(|a| {
                    let a = stem(a);
                    a == id || a == name || exe.as_ref() == Some(&a)
                }) || (!name.is_empty() && s.name.to_lowercase().contains(&name)))
        })
    }

    fn now_playing(&mut self, cx: &mut Context<Self>) -> Div {
        self.poll_sources();
        let Some(m) = cx.global::<PanelControl>().media.clone() else {
            let theme = Theme::current(cx);
            return components::card_panel(&theme)
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgba(theme.neutral_text_low))
                .child(i18n::t(cx, "dashboard.nothing_playing"));
        };
        // アプリのウィンドウを映す。無ければ、対応するプレイヤーだけに前面化を頼む。
        if self.open_app && self.pending.is_none() {
            self.open_app = false;
            let window = self.app_window(&m.app);
            match window
                .map(|(source, icon)| (source, icon, mirror_of(source, cx).map(|(id, _)| id)))
            {
                // もう映していれば新しく開かない。ダッシュボードに隠している配信なら、それを出す。
                Some((_, _, Some(id))) => {
                    let _ = self.dashboard.update(cx, |d, cx| d.show_stream(id, cx));
                }
                Some((source, icon, None)) => {
                    let docked = ui::docked(cx) == Some(self.panel);
                    let spec = mirror_spec(
                        source.clone(),
                        icon.clone(),
                        Some(self.dashboard.clone()),
                        self.panel,
                        false,
                        docked,
                    );
                    cx.default_global::<PanelControl>().spawns.push(spec);
                }
                None if m.controls.raise => {
                    commands(self.panel, [PanelCommand::Media(media::Command::Raise)], cx)
                }
                None => {}
            }
        }
        let place = |i: &Option<media::Image>| i.as_ref().map_or(0, |i| i.rgba.as_ptr() as usize);
        let key = [place(&m.art), place(&m.backdrop), place(&m.app.icon)];
        if self.media_images.as_ref().is_none_or(|i| i.key != key) {
            let convert = |i: &Option<media::Image>| {
                i.as_ref()
                    .and_then(|i| rgba_image(i.width, i.height, &i.rgba))
            };
            self.media_images = Some(MediaImages {
                key,
                art: convert(&m.art),
                backdrop: convert(&m.backdrop),
                icon: convert(&m.app.icon),
            });
        }
        let images = self.media_images.as_ref().expect("made above");
        // 背景がジャケットで変わるので、明るい背景には暗い文字（ライト）、暗い背景には明るい文字（ダーク）を載せる。
        let theme = match m.light {
            Some(true) => Theme::LIGHT,
            Some(false) => Theme::DARK,
            None => Theme::current(cx),
        };
        let low = theme.neutral_text_low;
        // Ghost ボタンの背景は、背景に紛れないよう反対のテーマの色にする（明るい背景には暗く、暗い背景には明るく）。
        let contrast = if theme == Theme::LIGHT {
            Theme::DARK
        } else {
            Theme::LIGHT
        };
        let app_window = self.app_window(&m.app);
        let can_open_app = app_window.is_some() || m.controls.raise;
        let icon = images
            .icon
            .clone()
            .or_else(|| app_window.and_then(|(_, i)| i.clone()));
        let panel = self.panel;
        let send = move |command| {
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                commands(panel, [PanelCommand::Media(command)], cx)
            }
        };
        let control = |id: &'static str, glyph, key, enabled: bool, command| {
            components::icon_button(&theme, id, glyph, i18n::t(cx, key), Variant::Ghost)
                .ghost_background(&contrast)
                .background_opacity(NOW_PLAYING_BACKGROUND_OPACITY)
                .when(!enabled, |b| b.opacity(0.4))
                .when(enabled, |b| b.on_click(send(command)))
        };
        let (position, duration) = (m.position_now(), m.duration);
        let fraction = duration.map_or(0.0, |d| {
            (position.as_secs_f32() / d.as_secs_f32().max(0.001)).clamp(0.0, 1.0)
        });
        let seek_bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let bounds_in = seek_bounds.clone();
        components::card_panel(&theme)
            .when_some(images.backdrop.clone(), |d, backdrop| {
                d.child(
                    img(backdrop)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover)
                        .rounded(px(design::radius::XL)),
                )
            })
            // ぼかしたジャケットの上に地の色を薄く重ね、文字と部品を読みやすくする。
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(design::radius::XL))
                    .bg(rgba(theme.background & !0xff | NOW_PLAYING_VEIL)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(spacing::LG))
                    .left(px(spacing::LG))
                    .child(
                        components::content_button(&theme, "now-playing-app", Variant::Ghost)
                            .ghost_background(&contrast)
                            .background_opacity(NOW_PLAYING_BACKGROUND_OPACITY)
                            .tooltip(i18n::t(
                                cx,
                                if can_open_app {
                                    "dashboard.open_app"
                                } else {
                                    "dashboard.app_unavailable"
                                },
                            ))
                            .when(!can_open_app, |b| b.opacity(0.4))
                            .child(match icon {
                                Some(icon) => img(icon).size(px(font_size::LG)).into_any_element(),
                                None => design::icon(&icons::APP_WINDOW)
                                    .size(px(font_size::LG))
                                    .into_any_element(),
                            })
                            .child(m.app.name.clone())
                            .when(can_open_app, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| {
                                    this.open_app = true;
                                    this.refresh();
                                    redraw(this.panel, cx);
                                }))
                            }),
                    ),
            )
            // メディアが複数あるときだけ、右上に前後のメディアへの切り替えを出す。
            .when(m.sessions > 1, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(spacing::LG))
                        .right(px(spacing::LG))
                        .flex()
                        .gap(px(spacing::SM))
                        .child(control(
                            "now-playing-previous-media",
                            &icons::CARET_LEFT,
                            "dashboard.previous_media",
                            true,
                            media::Command::Switch(false),
                        ))
                        .child(control(
                            "now-playing-next-media",
                            &icons::CARET_RIGHT,
                            "dashboard.next_media",
                            true,
                            media::Command::Switch(true),
                        )),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(spacing::SM))
                    .text_color(rgba(theme.neutral_text))
                    .child(match images.art.clone() {
                        Some(art) => img(art)
                            .size(px(ART_DISPLAY_PX))
                            .rounded(px(design::radius::LG))
                            .into_any_element(),
                        None => div()
                            .size(px(ART_DISPLAY_PX))
                            .rounded(px(design::radius::LG))
                            .bg(rgba(theme.control))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(design::icon(&icons::MUSIC_NOTES).size(px(TILE_ICON_PX)))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .debug_selector(|| "now-playing-title".into())
                            .mt(px(spacing::MD))
                            .max_w(px(SEEK_PX))
                            .truncate()
                            .text_size(px(font_size::LG))
                            .font_weight(FontWeight::BOLD)
                            .child(m.title.clone()),
                    )
                    .child(
                        div()
                            .max_w(px(SEEK_PX))
                            .truncate()
                            .text_color(rgba(low))
                            .child(m.artist.clone()),
                    )
                    // 1 行に、再生位置・シークバー・曲の長さ。
                    .child(
                        div()
                            .mt(px(spacing::MD))
                            .w(px(SEEK_PX))
                            .flex()
                            .items_center()
                            .gap(px(spacing::SM))
                            .text_size(px(font_size::SM))
                            .text_color(rgba(low))
                            .child(
                                div()
                                    .flex_none()
                                    .debug_selector(|| "now-playing-position".into())
                                    .child(clock(position)),
                            )
                            // シークバー: ハンドルの無い無彩色の棒。押した位置へ移る（シークできるときだけ）。
                            .child(
                                div()
                                    .id("now-playing-seek")
                                    .debug_selector(|| "now-playing-seek".into())
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(SEEK_HIT_PX))
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .child(
                                        gpui::canvas(
                                            move |bounds, _, _| bounds_in.set(bounds),
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .inset_0(),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(SEEK_BAR_PX))
                                            .rounded(px(design::radius::FULL))
                                            .bg(rgba(theme.neutral_text_low & !0xff | 0x55))
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w(gpui::relative(fraction))
                                                    .rounded(px(design::radius::FULL))
                                                    .bg(rgba(theme.neutral_text)),
                                            ),
                                    )
                                    .when_some(
                                        duration.filter(|_| m.controls.seek),
                                        |d, duration| {
                                            d.on_mouse_down(
                                                gpui::MouseButton::Left,
                                                move |e, _, cx| {
                                                    let b = seek_bounds.get();
                                                    let at = f32::from(e.position.x - b.origin.x)
                                                        / f32::from(b.size.width).max(1.0);
                                                    let to = duration.mul_f32(at.clamp(0.0, 1.0));
                                                    commands(
                                                        panel,
                                                        [PanelCommand::Media(
                                                            media::Command::Seek(to),
                                                        )],
                                                        cx,
                                                    );
                                                },
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .debug_selector(|| "now-playing-duration".into())
                                    .child(duration.map_or_else(|| "--:--".to_owned(), clock)),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(spacing::SM))
                            .flex()
                            .gap(px(spacing::LG))
                            .child(control(
                                "now-playing-previous",
                                &icons::SKIP_BACK,
                                "dashboard.previous",
                                m.controls.previous,
                                media::Command::Previous,
                            ))
                            .child(control(
                                "now-playing-play-pause",
                                if m.playing {
                                    &icons::PAUSE
                                } else {
                                    &icons::PLAY
                                },
                                if m.playing {
                                    "dashboard.pause"
                                } else {
                                    "dashboard.play"
                                },
                                m.controls.play_pause,
                                media::Command::PlayPause,
                            ))
                            .child(control(
                                "now-playing-next",
                                &icons::SKIP_FORWARD,
                                "dashboard.next",
                                m.controls.next,
                                media::Command::Next,
                            )),
                    ),
            )
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

    /// アプリ `app` のウィンドウ（取得元の一覧から。[`is_window_of`]）。
    fn window_for(&self, app: &InstalledApp) -> Option<&(Source, Option<Arc<RenderImage>>)> {
        self.sources.iter().find(|(s, _)| is_window_of(app, s))
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
    fn apps_page(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Div {
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
        let stem = |s: &str| {
            let s = s.to_ascii_lowercase();
            s.strip_suffix(".exe").map(str::to_owned).unwrap_or(s)
        };
        // 実行ファイル名（小文字・.exe 抜き）→ アプリ。ウィンドウごとに全アプリを探さないよう、先に一度だけ作る。
        let by_exe: std::collections::HashMap<String, &(InstalledApp, Option<Arc<RenderImage>>)> =
            self.apps
                .iter()
                .filter_map(|a| Some((stem(a.0.exe.as_deref()?), a)))
                .collect();
        let mut running: Vec<RunningApp> = Vec::new();
        for (source, image) in &self.sources {
            if mirror_of(source, cx).is_some_and(|(_, resident)| resident) {
                continue;
            }
            let key = source
                .app
                .as_deref()
                .map_or_else(|| source.name.clone(), stem);
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
                        redraw(this.panel, cx);
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
                    redraw(this.panel, cx);
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
                    redraw(this.panel, cx);
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
                redraw(this.panel, cx);
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
                        redraw(this.panel, cx);
                    })),
                )
            })
    }

    fn quick_settings(
        &self,
        theme: &Theme,
        time_width: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Div {
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
                                        .child(fixed_clock(time_width)),
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

/// 配信を映すパネル。画像はカーネルが直接送るので、描くのは始まるまで・終わった後の状態と、起動したアプリのウィンドウを
/// 待っている間（[`launch_spec`]）だけ。映しているウィンドウが閉じたら、パネルも閉じる。
struct Mirror {
    panel: PanelId,
    /// 映しているもの（ウィンドウを待っている間は `None`）。
    source: Option<Source>,
    /// 起動したアプリのウィンドウを待っている。
    waiting: Option<Waiting>,
    /// 映し始めたら配信のボタンを足し、最小化時にしまうダッシュボード。
    dashboard: Option<WeakEntity<Dashboard>>,
    /// 配信の再接続を探している（状態が書き戻されるたびに繰り返さない）。
    ending: bool,
    was_hidden: bool,
    watch_generation: u64,
}

/// 起動したアプリのウィンドウを待っている間の中身。
struct Waiting {
    app: InstalledApp,
    icon: Option<Arc<RenderImage>>,
    /// 待つのをやめた（[`LAUNCH_WAIT`] の間に出なかった）。
    gave_up: bool,
}

impl Mirror {
    fn new(
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
            was_hidden: false,
            watch_generation: 0,
        };
        if this.waiting.is_some() {
            this.wait_for_window(cx);
        } else {
            this.watch_successor(cx);
        }
        this
    }

    /// 最小化されたパネルはしまい、取得元が入れ替わったら配信をつなぎ直す。
    fn follow_status(&mut self, cx: &mut Context<Self>) {
        let panel_status = cx
            .global::<PanelControl>()
            .panels
            .get(&self.panel)
            .map(|p| &p.status);
        let status = panel_status.and_then(|s| s.desktop.clone());
        let hidden = panel_status.is_some_and(|s| s.hidden);
        if self.was_hidden && !hidden && matches!(status, Some(Status::Minimized)) {
            self.was_hidden = false;
            if let Some(source) = self.source.clone() {
                self.restart(source, cx);
            }
            return;
        }
        self.was_hidden = hidden;
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
                redraw(self.panel, cx);
                if let Some(dashboard) = &self.dashboard {
                    let _ = dashboard.update(cx, |d, cx| d.minimize(self.panel, cx));
                } else {
                    close(self.panel, cx);
                }
            }
            Some(Status::Closed | Status::Failed(_))
                if self
                    .source
                    .as_ref()
                    .is_some_and(|s| s.kind == SourceKind::Window) =>
            {
                self.ending = true;
                redraw(self.panel, cx);
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
        commands(
            self.panel,
            [PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE)],
            cx,
        );
        redraw(self.panel, cx);
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
                    && let Some(next) = successor(&source, &known, &sources)
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
                                .and_then(|s| s.into_iter().find(|s| is_window_of(&app, s)));
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
                        redraw(m.panel, cx);
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
    fn found(&mut self, source: Source, cx: &mut Context<Self>) {
        let icon = self.waiting.take().and_then(|w| w.icon);
        self.source = Some(source.clone());
        start_mirror(self.panel, source, icon, self.dashboard.clone(), false, cx);
        redraw(self.panel, cx);
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

/// `source` がアプリ `app` のウィンドウか。実行ファイル名で見て、分からないアプリ（ストアのアプリなど）は
/// ウィンドウの名前にアプリの名前を含むかで見る。
fn is_window_of(app: &InstalledApp, source: &Source) -> bool {
    source.kind == SourceKind::Window
        && match &app.exe {
            Some(exe) => source
                .app
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(exe)),
            None => {
                let name = app.name.to_lowercase();
                !name.is_empty() && source.name.to_lowercase().contains(&name)
            }
        }
}

/// 元のウィンドウが消えた後、同じアプリの後継ウィンドウを選ぶ。
fn replacement(source: &Source, sources: &[Source]) -> Option<Source> {
    let app = source.app.as_deref().unwrap_or("");
    let title = source.name.to_lowercase();
    let app_name = app.to_ascii_lowercase();
    let app_name = app_name.strip_suffix(".exe").unwrap_or(&app_name);
    // shortcut: タイトルと実行ファイル名が結び付かない後継窓は追えない。実例で外れたらプロセスの親子関係も調べる。
    sources
        .iter()
        .filter(|s| s.kind == SourceKind::Window && !s.same_target(source))
        .find(|s| {
            let candidate = s.name.to_lowercase();
            title.len() >= 5 && candidate.contains(&title)
                || app_name.len() >= 5
                    && title.contains(app_name)
                    && candidate.contains(app_name)
                    && s.app
                        .as_deref()
                        .is_some_and(|a| a.eq_ignore_ascii_case(app))
        })
        .cloned()
}

fn successor(source: &Source, known: &[Source], current: &[Source]) -> Option<Source> {
    let still_open = current.iter().any(|s| s.same_target(source));
    replacement(
        source,
        &current
            .iter()
            .filter(|s| !still_open || !known.iter().any(|k| k.same_target(s)))
            .cloned()
            .collect::<Vec<_>>(),
    )
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
    commands(
        panel,
        [PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE)],
        cx,
    );
    if let Some(dashboard) = dashboard {
        let _ = dashboard.update(cx, |d, cx| {
            d.add_stream(panel, kind, name, icon, resident, cx)
        });
    }
}

/// 起動したアプリ `app` の画面: 開いた時点ではアイコンと名前と「起動しています」を出して待ち、ウィンドウが出たらそこで
/// 映し始める（[`Mirror::wait_for_window`]）。`dashboard` があればはめて開く（ランチャーと入れ替わる）。
fn launch_spec(
    app: InstalledApp,
    icon: Option<Arc<RenderImage>>,
    dashboard: Option<WeakEntity<Dashboard>>,
    from: PanelId,
    docked: bool,
) -> PanelSpec {
    let title = app.name.clone();
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
    if desktop::capabilities().minimize {
        spec.source_minimizable()
    } else {
        spec
    }
}

/// 開いている配信のパネルと、その取得元・常駐か。同じ画面・ウィンドウは 1 枚だけ映す（[`mirror_of`]）。
#[derive(Default)]
struct Mirrors(Vec<(Source, PanelId, bool)>);

impl gpui::Global for Mirrors {}

/// `source` を映している、閉じていないパネルと、それが常駐（起動時に用意した画面）か。
// ponytail: 開くのは次のフレームなので、同じフレームに 2 回押すと 2 枚開く。困れば開く前の分も数える。
fn mirror_of(source: &Source, cx: &App) -> Option<(PanelId, bool)> {
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
fn mirror_spec(
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
    if source_kind == SourceKind::Window && desktop::capabilities().minimize {
        spec.source_minimizable()
    } else {
        spec
    }
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
        .h(px(TILE_PX))
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
    let run = gpui::TextRun {
        len: name.len(),
        font: design::font::font(),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .shape_line(name.to_owned().into(), px(font_size::MD), &[run], None)
            .width,
    ) > available
}

/// アプリのアイコンの画像。無ければ `glyph`。
fn tile_image(icon: Option<Arc<RenderImage>>, glyph: &'static design::Icon) -> gpui::AnyElement {
    match icon {
        Some(icon) => img(icon).size(px(TILE_ICON_PX)).into_any_element(),
        None => tile_icon(glyph),
    }
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
    rgba_image(icon.width, icon.height, &icon.rgba)
}

/// RGBA の画素を GPUI の画像（BGRA）にする。
fn rgba_image(width: u32, height: u32, rgba: &[u8]) -> Option<Arc<RenderImage>> {
    let mut bgra = rgba.to_vec();
    for p in bgra.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// 時刻（`時:分`）を太字の `size` px で組んだときの、どの数字でも収まる幅。いちばん広い数字（"00:00"〜"99:99"）で測る。
fn clock_width(window: &Window, size: f32) -> gpui::Pixels {
    let font = gpui::Font {
        weight: FontWeight::BOLD,
        ..design::font::font()
    };
    (0..10)
        .map(|d| {
            let text: SharedString = format!("{d}{d}:{d}{d}").into();
            let run = gpui::TextRun {
                len: text.len(),
                font: font.clone(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(text, px(size), &[run], None)
                .width
        })
        .fold(px(0.0), |a, b| a.max(b))
}

/// 今の時刻（`時:分`、太字）を、幅 `width` の箱の中央に置く。数字で幅が変わって周りが動かないように。
fn fixed_clock(width: gpui::Pixels) -> Div {
    div()
        .flex_none()
        .w(width)
        .flex()
        .justify_center()
        .font_weight(FontWeight::BOLD)
        .child(chrono::Local::now().format("%H:%M").to_string())
}

/// 再生位置・長さを `分:秒`（1 時間を超えれば `時:分:秒`）で。
fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
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
        assert!(std::ptr::eq(
            battery_glyph(&battery(1.0, false)),
            &icons::BATTERY_FULL
        ));
        assert!(std::ptr::eq(
            battery_glyph(&battery(0.5, false)),
            &icons::BATTERY_MEDIUM
        ));
        assert!(std::ptr::eq(
            battery_glyph(&battery(0.05, false)),
            &icons::BATTERY_EMPTY
        ));
        assert!(std::ptr::eq(
            battery_glyph(&battery(0.05, true)),
            &icons::BATTERY_CHARGING
        ));
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
        assert!(
            s.position.abs_diff_eq(SCREEN_POSE.position, 1e-3),
            "{}",
            s.position
        );
        assert!(s.orientation.abs_diff_eq(Quat::IDENTITY, 1e-4));
        let b = ov.panel(bar).pose.position;
        assert!(b.y < -0.3 && b.length() >= 1.0, "{b}");
    }

    fn summon(from: PanelId, opened: Opened) -> PanelSpec {
        PanelSpec::spawn(from, move |id, _, cx| {
            opened(id, cx);
            Mirror::new(id, None, None, None, cx)
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

    /// `id` に積まれた操作を取り出す（[`commands`] は写すだけ）。
    fn take_commands(cx: &mut VisualTestContext, id: PanelId) -> Vec<PanelCommand> {
        cx.update(|_, cx| {
            std::mem::take(
                &mut cx
                    .default_global::<PanelControl>()
                    .panels
                    .entry(id)
                    .or_default()
                    .commands,
            )
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
        assert_eq!(
            cx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            0
        );
        // 設定を選ぶと（メニューから。`menu_items_open_screens_and_close_the_menu`）、main が渡した設定画面がバーの上にはまる。
        bar.update(cx, |d, cx| d.toggle(Page::Settings, cx));
        open_spawned(cx, 2);
        assert!(commands(cx, 2).iter().any(docked_to_bar));
        assert_eq!(
            cx.update(|_, cx| bar.read(cx).current(cx)),
            Some(Page::Settings)
        );
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
    }

    /// メニューボタンはバーに固定したポップオーバーを開き、その項目を選ぶと画面をはめてメニューを閉じる。
    #[gpui::test]
    fn menu_items_open_screens_and_close_the_menu(cx: &mut TestAppContext) {
        let bar = cx
            .add_window(|_, cx| {
                cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
                Dashboard::new(1, summon, summon, cx)
            })
            .root(cx)
            .unwrap();
        let (_bar_view, bcx) = cx.add_window_view({
            let bar = bar.clone();
            move |_, _| BarView(bar)
        });
        bcx.run_until_parked();
        click(bcx, "dashboard-menu");
        let spec = bcx.update(|_, cx| cx.default_global::<PanelControl>().spawns.pop().unwrap());
        assert!(
            spec.parent()
                .is_some_and(|p| p.panel == 1 && p.docked && p.dismiss)
        );
        let (_menu, mcx) = cx.add_window_view(|_, _| Menu {
            panel: 9,
            dashboard: bar.downgrade(),
        });
        mcx.run_until_parked();
        click(mcx, "menu-settings");
        assert!(commands(mcx, 9).contains(&PanelCommand::Close));
        assert_eq!(
            mcx.update(|_, cx| cx.global::<PanelControl>().spawns.len()),
            1
        );
    }

    /// 終了の確かめのダイアログで終了を選ぶとアプリを終え、キャンセルならダイアログだけ閉じる。
    #[gpui::test]
    fn quit_dialog_quits_or_cancels(cx: &mut TestAppContext) {
        let (_dialog, cx) = cx.add_window_view(|_, _| QuitDialog { panel: 7 });
        cx.run_until_parked();
        click(cx, "quit-cancel");
        assert!(commands(cx, 7).contains(&PanelCommand::Close));
        assert!(!cx.update(|_, cx| cx.global::<PanelControl>().quit));
        click(cx, "quit-confirm");
        assert!(cx.update(|_, cx| cx.global::<PanelControl>().quit));
    }

    fn playing() -> std::sync::Arc<media::NowPlaying> {
        std::sync::Arc::new(media::NowPlaying {
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            playing: false,
            position: Duration::from_secs(65),
            duration: Some(Duration::from_secs(200)),
            at: std::time::Instant::now(),
            app: media::App {
                id: "Spotify.exe".into(),
                name: "Spotify".into(),
                icon: None,
                exe: None,
            },
            art: None,
            backdrop: None,
            light: Some(true),
            controls: media::Controls {
                play_pause: true,
                next: true,
                previous: false,
                seek: true,
                raise: false,
            },
            sessions: 1,
        })
    }

    /// 何か再生しているときだけ、バーに再生中の画面のボタンが出る。
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

    /// 進行中の作業の知らせは、バーのクイック設定と再生中の間に出る。押すと出したニャプリがはまり、
    /// 終わったカウントダウンの知らせは片付く。
    #[gpui::test]
    fn activities_sit_between_quick_settings_and_now_playing(cx: &mut TestAppContext) {
        let (_bar, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            cx.default_global::<PanelControl>().media = Some(playing());
            Dashboard::new(1, summon, summon, cx)
        });
        let manifest = nyapp::Manifest::parse(
            "id = \"net.bettaworx.clock\"\nname = \"Clock\"\nversion = \"1\"\napi = 1\n",
        )
        .unwrap();
        let clock = nyapp::Installed {
            manifest,
            path: std::env::temp_dir().join("net.bettaworx.clock.nyapp"),
            icon: Default::default(),
        };
        cx.update(|_, cx| {
            let ends = nyapp::activity::now_ms() + 90_000;
            let countdown = nyapp::Activity::Countdown {
                title: "Timer".into(),
                ends_at_ms: ends,
                total_ms: 120_000,
            };
            nyapp::activity::set(&clock, Some(countdown), cx);
        });
        redraw_bar(cx);
        let x = |cx: &mut VisualTestContext, s: &'static str| cx.debug_bounds(s).unwrap().origin.x;
        let (quick, activity, playing) = (
            x(cx, "dashboard-quick-settings"),
            x(cx, "dashboard-activity-net.bettaworx.clock"),
            x(cx, "dashboard-now-playing"),
        );
        let pill = cx
            .debug_bounds("dashboard-activity-net.bettaworx.clock")
            .unwrap();
        assert_eq!(pill.size.width, pill.size.height, "知らせは 1:1");
        assert!(
            quick < activity && activity < playing,
            "{quick:?} < {activity:?} < {playing:?}"
        );
        // 終わったものを押すと片付き、ニャプリがはまる。
        cx.update(|_, cx| {
            let live = nyapp::activity::all(cx)[0].clone();
            nyapp::activity::set(&live.app, None, cx);
            let ends = nyapp::activity::now_ms() - 1;
            let done = nyapp::Activity::Countdown {
                title: "Timer".into(),
                ends_at_ms: ends,
                total_ms: 1,
            };
            nyapp::activity::set(&live.app, Some(done), cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|_, cx| nyapp::activity::all(cx)[0].finished));
        redraw_bar(cx);
        click(cx, "dashboard-activity-net.bettaworx.clock");
        assert!(
            cx.update(|_, cx| nyapp::activity::all(cx).is_empty()),
            "終わったものは片付く"
        );
        open_spawned(cx, 4);
        assert!(
            commands(cx, 4).iter().any(docked_to_bar),
            "ニャプリがはまる"
        );
    }

    fn redraw_bar(cx: &mut VisualTestContext) {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// 再生中の画面: 曲と位置・長さを出し、操作ボタンとシークバーはカーネルへの操作を積む。
    #[gpui::test]
    fn now_playing_screen_shows_the_song_and_sends_commands(cx: &mut TestAppContext) {
        let (_screen, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().media = Some(playing());
            Screen::new(2, Page::NowPlaying, WeakEntity::new_invalid(), cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("now-playing-title").is_some());
        assert!(cx.debug_bounds("now-playing-position").is_some());
        click(cx, "now-playing-play-pause");
        click(cx, "now-playing-next");
        // 前へはできないので、押しても何も積まない。
        click(cx, "now-playing-previous");
        assert_eq!(
            commands(cx, 2),
            [
                PanelCommand::Media(media::Command::PlayPause),
                PanelCommand::Media(media::Command::Next),
            ]
        );
        // シークバーの右端近くを押すと、曲の終わり近くへ移る。
        let bar = cx.debug_bounds("now-playing-seek").unwrap();
        let at = gpui::point(bar.right() - px(1.0), bar.center().y);
        cx.simulate_click(at, Modifiers::none());
        let last = commands(cx, 2).pop();
        assert!(
            matches!(last, Some(PanelCommand::Media(media::Command::Seek(d))) if d > Duration::from_secs(190))
        );
    }

    /// メディアが 1 つなら切り替えを出さず、複数なら右上の前後のボタンで切り替えを積む。
    #[gpui::test]
    fn media_switch_shows_only_with_several_sessions(cx: &mut TestAppContext) {
        let (screen, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().media = Some(playing());
            Screen::new(2, Page::NowPlaying, WeakEntity::new_invalid(), cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("now-playing-next-media").is_none());
        cx.update(|_, cx| {
            let mut media = (*playing()).clone();
            media.sessions = 2;
            cx.default_global::<PanelControl>().media = Some(Arc::new(media));
        });
        screen.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        click(cx, "now-playing-previous-media");
        click(cx, "now-playing-next-media");
        let c = commands(cx, 2);
        assert!(c.contains(&PanelCommand::Media(media::Command::Switch(false))));
        assert!(c.contains(&PanelCommand::Media(media::Command::Switch(true))));
    }

    #[gpui::test]
    fn unavailable_app_window_is_not_clickable(cx: &mut TestAppContext) {
        let (screen, cx) = cx.add_window_view(|_, cx| {
            let mut media = (*playing()).clone();
            media.app.id = "nyaw-unavailable-test.exe".into();
            media.app.name = "nyaw-unavailable-test".into();
            cx.default_global::<PanelControl>().media = Some(Arc::new(media));
            Screen::new(2, Page::NowPlaying, WeakEntity::new_invalid(), cx)
        });
        cx.run_until_parked();
        click(cx, "now-playing-app");
        assert!(!screen.read_with(cx, |s, _| s.open_app));
        assert!(commands(cx, 2).is_empty());
    }

    /// テストでバーを別の窓に描くための包み。
    struct BarView(gpui::Entity<Dashboard>);
    impl Render for BarView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.clone()
        }
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

    /// 隠している間に押した分は、出し直したのと同じ状態の書き戻しで届いても隠さない（開く音だけ鳴って出ない）。
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
        // 出し直したのと同じフレームに、何も無い所を押した数も増えた。
        set(cx, 5, false);
        assert!(!commands(cx, 1).contains(&PanelCommand::Hide(true)));
        set(cx, 6, false);
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
        let icon = || {
            let rgba = vec![200u8; 64 * 64 * 4];
            rgba_image(64, 64, &rgba)
        };
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

    /// 最小化された窓の画面には「戻す」ボタンが出て、押すと同じ窓を映し直す（映し直しが最小化を戻す）。
    #[gpui::test]
    fn mirror_offers_restore_when_its_window_is_minimized(cx: &mut TestAppContext) {
        let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
        let (_, cx) = cx.add_window_view(|_, cx| Mirror::new(4, Some(source), None, None, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .desktop = Some(Status::Minimized)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("mirror-retry").is_none());
        assert!(cx.debug_bounds("mirror-restore").is_some());
        take_commands(cx, 4);
        click(cx, "mirror-restore");
        assert_eq!(
            take_commands(cx, 4),
            [PanelCommand::Stream(
                Some(kernel::desktop::mock::source("Editor", SourceKind::Window)),
                DEFAULT_FRAME_RATE
            )]
        );
    }

    /// 配信が終わってもパネルに再試行を出す。
    #[gpui::test]
    fn mirror_offers_retry_when_its_window_closes(cx: &mut TestAppContext) {
        let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
        let (_, cx) = cx.add_window_view(|_, cx| Mirror::new(4, Some(source), None, None, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .desktop = Some(Status::Closed)
        });
        cx.run_until_parked();
        assert!(take_commands(cx, 4).is_empty());
        assert!(cx.debug_bounds("mirror-retry").is_some());
        click(cx, "mirror-retry");
        assert_eq!(
            take_commands(cx, 4),
            [PanelCommand::Stream(
                Some(kernel::desktop::mock::source("Editor", SourceKind::Window)),
                DEFAULT_FRAME_RATE
            )]
        );
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .redraw = true
        });
        cx.run_until_parked();
        assert!(take_commands(cx, 4).is_empty());
    }

    #[gpui::test]
    fn mirror_offers_retry_when_capture_fails(cx: &mut TestAppContext) {
        let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
        let (_, cx) =
            cx.add_window_view(|_, cx| Mirror::new(4, Some(source.clone()), None, None, cx));
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .desktop = Some(Status::Failed(kernel::desktop::Error(
                "capture stopped".into(),
            )))
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("mirror-retry").is_some());
        click(cx, "mirror-retry");
        assert_eq!(
            take_commands(cx, 4),
            [PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE)]
        );
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
        if desktop::capabilities().minimize {
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
            assert_eq!(take_commands(ncx, 4), [PanelCommand::MinimizeSource]);
        }
    }

    /// テストで [`Mirror`] を窓に載せる入れ物。
    struct Waiter(gpui::Entity<Mirror>);

    impl Render for Waiter {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.clone()
        }
    }

    /// ダッシュボードを出し直すと、はまっている画面（配信なら、そのウィンドウ）を前に出すよう頼む。
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
        let [long, short] =
            ["dashboard-app-0", "dashboard-app-1"].map(|s| cx.debug_bounds(s).unwrap());
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

    /// 滑り込みの間の幅の計算（クイック設定の自然な幅）が、出きったときの実際の幅と合う。合わないと出きる瞬間に飛ぶ。
    #[gpui::test]
    fn quick_settings_width_matches_the_laid_out_button(cx: &mut TestAppContext) {
        for with_battery in [false, true] {
            let (_bar, cx) = cx.add_window_view(|_, cx| {
                cx.default_global::<PanelControl>().batteries = if with_battery {
                    vec![battery(0.9, false)]
                } else {
                    vec![]
                };
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
            let expected = quick_settings_width(f32::from(time), with_battery);
            assert!(
                (actual - expected).abs() <= 1.0,
                "電池 {with_battery}: {actual} / {expected}"
            );
        }
    }

    /// 配信が溜まって最大幅を超えそうでも、バーはそこで頭打ちになり、横にスクロールできる。
    #[gpui::test]
    fn bar_stops_at_the_max_width_and_scrolls(cx: &mut TestAppContext) {
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
        assert!(width <= MAX_BAR_PX + 1.0, "頭打ち: {width}");
        assert!(bar.read_with(cx, |d, _| d.scroll.max_offset().x) > px(0.0));
    }

    /// バーは窓いっぱいに広がらず、中身の幅に収まって中央に置かれる。
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
            "窓いっぱいに広がらない: {content:?} / {width:?}"
        );
        let (left, right) = (first.left(), width - last.right());
        assert!(
            (left - right).abs() <= px(2.0),
            "中央に置く: {left:?} / {right:?}"
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
            redraw(2, cx);
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

    /// 開いた配信はバーに溜まり、押すとはまる。はまっていた配信は閉じずに隠して外し、もう一度押すと隠す。
    /// 閉じた配信のボタンは消える。
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
        // 配信は区切り線の右に、押せるボタンとして並ぶ。
        assert!(cx.debug_bounds("dashboard-stream-4").is_some());
        click(cx, "dashboard-stream-5");
        assert_eq!(
            commands(cx, 4)[1..],
            [PanelCommand::Hide(true), PanelCommand::Parent(None)]
        );
        // 隠したままはめてから出す。
        let five = commands(cx, 5);
        assert!(docked_to_bar(&five[0]));
        assert_eq!(five[1], PanelCommand::FitHeight(SIZE_M.y));
        assert_eq!(
            five[3],
            PanelCommand::FocusSource,
            "出したウィンドウを前に出す"
        );
        assert!(
            !commands(cx, 4).contains(&PanelCommand::Close),
            "隠すだけで閉じない"
        );
        // もう一度押すと隠す。
        click(cx, "dashboard-stream-5");
        assert!(commands(cx, 5).contains(&PanelCommand::Hide(true)));
        // 閉じた配信のボタンは消える。
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

    #[test]
    fn replacement_prefers_matching_title_when_an_app_has_multiple_windows() {
        let mut original = kernel::desktop::mock::source("Project", SourceKind::Window);
        original.app = Some("Adobe.exe".into());
        let mut other = kernel::desktop::mock::source("Other", SourceKind::Window);
        other.app = Some("Adobe.exe".into());
        let mut next = kernel::desktop::mock::source("Project", SourceKind::Window);
        next.app = Some("adobe.exe".into());
        assert_eq!(replacement(&original, &[other, next.clone()]), Some(next));
    }

    #[test]
    fn replacement_does_not_switch_to_an_unrelated_chrome_window() {
        let mut original = kernel::desktop::mock::source("Tab A", SourceKind::Window);
        original.app = Some("chrome.exe".into());
        let mut other = kernel::desktop::mock::source("Tab B", SourceKind::Window);
        other.app = Some("chrome.exe".into());
        assert_eq!(replacement(&original, &[other]), None);
    }

    #[test]
    fn replacement_follows_an_adobe_window_across_executables() {
        let mut original = kernel::desktop::mock::source("Adobe Photoshop", SourceKind::Window);
        original.app = Some("AdobeLauncher.exe".into());
        let mut next =
            kernel::desktop::mock::source("Untitled - Adobe Photoshop", SourceKind::Window);
        next.app = Some("Photoshop.exe".into());
        assert_eq!(replacement(&original, &[next.clone()]), Some(next));
    }

    #[test]
    fn successor_tracks_a_new_adobe_window_while_the_splash_stays_open() {
        use kernel::desktop::mock::with_target;
        let mut splash = with_target(
            kernel::desktop::mock::source("Adobe Photoshop", SourceKind::Window),
            1,
        );
        splash.app = Some("Photoshop.exe".into());
        let mut previous = with_target(
            kernel::desktop::mock::source("Another - Adobe Photoshop", SourceKind::Window),
            2,
        );
        previous.app = Some("Photoshop.exe".into());
        let mut opened = with_target(
            kernel::desktop::mock::source("Untitled - Adobe Photoshop", SourceKind::Window),
            3,
        );
        opened.app = Some("Photoshop.exe".into());
        assert_eq!(
            successor(
                &splash,
                &[splash.clone(), previous.clone()],
                &[splash.clone(), previous, opened.clone()]
            ),
            Some(opened)
        );
    }

    #[test]
    fn successor_does_not_replace_chrome_with_an_existing_window() {
        use kernel::desktop::mock::with_target;
        let mut chosen = with_target(
            kernel::desktop::mock::source("New Tab - Google Chrome", SourceKind::Window),
            1,
        );
        chosen.app = Some("chrome.exe".into());
        let mut other = with_target(
            kernel::desktop::mock::source("New Tab - Google Chrome", SourceKind::Window),
            2,
        );
        other.app = Some("chrome.exe".into());
        assert_eq!(
            successor(
                &chosen,
                &[chosen.clone(), other.clone()],
                &[chosen.clone(), other]
            ),
            None
        );
    }

    #[gpui::test]
    fn reconnecting_updates_the_mirrored_window_and_stream(cx: &mut TestAppContext) {
        let old = kernel::desktop::mock::source("Adobe Photoshop", SourceKind::Window);
        let next = kernel::desktop::mock::source("Untitled - Adobe Photoshop", SourceKind::Window);
        let (mirror, cx) = cx.add_window_view(|_, cx| {
            cx.set_global(Mirrors(vec![(old.clone(), 4, false)]));
            Mirror::new(4, Some(old), None, None, cx)
        });
        mirror.update(cx, |mirror, cx| mirror.restart(next.clone(), cx));
        assert_eq!(
            take_commands(cx, 4),
            [PanelCommand::Stream(Some(next.clone()), DEFAULT_FRAME_RATE)]
        );
        assert_eq!(
            cx.update(|_, cx| cx.global::<Mirrors>().0[0].0.name.clone()),
            next.name
        );
    }

    #[gpui::test]
    fn minimized_window_is_stashed_and_restarted_when_shown(cx: &mut TestAppContext) {
        let source = kernel::desktop::mock::source("Editor", SourceKind::Window);
        let bar = cx
            .add_window(|_, cx| {
                cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
                Dashboard::new(1, summon, summon, cx)
            })
            .root(cx)
            .unwrap();
        let spec = mirror_spec(source.clone(), None, Some(bar.downgrade()), 2, false, false);
        let (_mirror, cx) = cx.add_window_view(|window, cx| {
            Waiter(spec.build(4, window, cx).downcast::<Mirror>().unwrap())
        });
        assert_eq!(
            take_commands(cx, 4),
            [PanelCommand::Stream(
                Some(source.clone()),
                DEFAULT_FRAME_RATE
            )],
            "独立表示ではバーにドッキングしない"
        );
        assert_eq!(bar.read_with(cx, |d, _| d.streams.len()), 1);
        cx.update(|_, cx| {
            let panel = cx
                .default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default();
            panel.status.desktop = Some(Status::Minimized);
        });
        cx.run_until_parked();
        assert!(take_commands(cx, 4).contains(&PanelCommand::Hide(true)));
        assert!(bar.read_with(cx, |d, _| d.streams[0].stashed));
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .hidden = true
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(4)
                .or_default()
                .status
                .hidden = false
        });
        cx.run_until_parked();
        assert!(
            take_commands(cx, 4).contains(&PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE))
        );
    }

    /// 常駐の配信は、独立させてもバーに残り、しまうと閉じずに隠れる。
    #[gpui::test]
    fn resident_streams_stay_on_the_bar_and_minimize(cx: &mut TestAppContext) {
        let (bar, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        });
        bar.update(cx, |d, cx| {
            d.add_stream(4, SourceKind::Screen, "Display 1".into(), None, true, cx)
        });
        // はめてから自分で外した（独立させた）。
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
            "独立させても残る"
        );
        bar.update(cx, |d, cx| d.minimize(4, cx));
        let c = commands(cx, 4);
        assert!(c.contains(&PanelCommand::Hide(true)));
        assert!(!c.contains(&PanelCommand::Close), "閉じない");
        assert!(bar.read_with(cx, |d, _| d.streams[0].stashed));
    }
}
