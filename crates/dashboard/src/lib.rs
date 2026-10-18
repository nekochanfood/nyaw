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
    App, Context, Div, FontWeight, RenderImage, ScrollHandle, SharedString, WeakEntity, Window, div,
    img, prelude::*, px, rgba,
};
use kernel::audio::Sound;
use kernel::desktop::{DEFAULT_FRAME_RATE, Icon, Source, SourceKind, Sources, Status};
use kernel::subsystems::media;
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
/// 再生中の画面: ジャケットの一辺、シークバーの幅・棒の太さ・押せる高さ（px）、背景に重ねる地の濃さ（0..=255）。
const ART_DISPLAY_PX: f32 = 280.0;
const SEEK_PX: f32 = 560.0;
const SEEK_BAR_PX: f32 = 6.0;
const SEEK_HIT_PX: f32 = 32.0;
const NOW_PLAYING_VEIL: u32 = 0x8c;
/// これより少ない残量は危険の色にする。
const LOW_BATTERY: f32 = 0.2;

/// 開くとき・閉じるときの音。どちらもダッシュボード自身から聞こえる（閉じる音は閉じ方を問わず鳴る）。
pub const OPEN_SOUND: &[u8] = include_bytes!("../assets/dashboard_open.wav");
pub const CLOSE_SOUND: &[u8] = include_bytes!("../assets/dashboard_close.wav");

/// ダッシュボードから開くほかの画面（デバッグ・設定）の起動仕様を作る。第 1 引数は開いた元のパネル
/// （[`PanelSpec::spawn`] に渡す）。開いたら引数で PanelId を知らせる（[`spec`] と同じ形）。
/// 画面どうしを直接つながないよう、何を開くかは main が選ぶ。
pub type Summon = fn(PanelId, Box<dyn FnOnce(PanelId, &mut App)>) -> PanelSpec;

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

/// ダッシュボード（バー）の起動仕様。`opened` は作られたパネルの ID を受け取る。`hidden` なら隠したまま開く
/// （起動時に作っておき、画面の配信を用意しておく。出すのは [`show`]）。
pub fn spec(
    opened: impl FnOnce(PanelId, &mut App) + 'static,
    settings: Summon,
    debug: Summon,
    hidden: bool,
) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, move |id, _, cx: &mut Context<Dashboard>| {
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
    /// 再生中のメディア（[`Screen`]）。
    NowPlaying,
    Mirror,
    Debug,
    Settings,
}

/// ダッシュボード（バー）。左からメニュー（デバッグ・設定のポップオーバー）・時刻と電池（クイック設定）・画面やウィンドウを
/// 開く「+」、区切り線を挟んで開いた配信のボタン（溜まっていき、はみ出すと横にスクロールする）。
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
}

impl Dashboard {
    fn new(panel: PanelId, settings: Summon, debug: Summon, cx: &mut Context<Self>) -> Self {
        tick_every_second(panel, cx);
        cx.observe_global::<PanelControl>(Self::close_on_outside_press)
            .detach();
        // 画面（モニター）の一覧が届いたら、すべて配信として開いておく。隠している間は描かないので、描画とは別に待つ。
        if let Ok(pending) = Sources::refresh() {
            cx.spawn(async move |this, cx| {
                let sources = loop {
                    if let Some(result) = pending.poll() {
                        break result;
                    }
                    cx.background_executor().timer(Duration::from_millis(100)).await;
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
                Menu { panel: id, dashboard }
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
            if source.kind == SourceKind::Screen && source.concrete() && mirror_of(&source, cx).is_none() {
                let spec = mirror_spec(source, None, Some(cx.weak_entity()), self.panel, true);
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
            commands(id, [PanelCommand::Hide(false)], cx);
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
                Page::QuickSettings | Page::NowPlaying | Page::Mirror => {
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

    /// `panel` に `page` をはめた。
    fn track(&mut self, page: Page, panel: PanelId, cx: &mut Context<Self>) {
        self.shown = Some((page, panel));
        redraw(self.panel, cx);
    }
}

/// ダッシュボードで開いた配信。
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
        let quit = components::menu_item(&theme, "menu-quit", &icons::POWER, i18n::t(cx, "dashboard.quit"))
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
                    item("menu-settings", &icons::GEAR, "dashboard.settings", Page::Settings),
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
                components::button(&theme, "quit-cancel", i18n::t(cx, "common.cancel"), Variant::Default)
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
        let theme = Theme::current(cx);
        let current = self.current(cx);
        let docked = ui::docked(cx);
        let streams: Vec<_> = self
            .streams
            .iter()
            .map(|s| {
                let id = s.panel;
                let variant = if docked == Some(id) {
                    Variant::Primary
                } else {
                    Variant::Default
                };
                let glyph = match (s.kind, &s.icon) {
                    (SourceKind::Window, Some(icon)) => img(icon.clone())
                        .size(px(font_size::LG))
                        .into_any_element(),
                    (SourceKind::Window, None) => design::icon(&icons::APP_WINDOW)
                        .weight(design::Weight::Bold)
                        .size(px(font_size::LG))
                        .into_any_element(),
                    (SourceKind::Screen, _) => design::icon(&icons::MONITOR)
                        .weight(design::Weight::Bold)
                        .size(px(font_size::LG))
                        .into_any_element(),
                };
                components::round_button(&theme, format!("dashboard-stream-{id}"), s.name.clone(), variant)
                    .flex_none()
                    .child(glyph)
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_stream(id, cx)))
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
        let has_media = ctl.media.is_some();
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
            &icons::PLUS,
            i18n::t(cx, "dashboard.add_screen"),
            Page::Mirror,
        );
        let now_playing = page_button(
            "dashboard-now-playing",
            &icons::PLAYLIST,
            i18n::t(cx, "dashboard.now_playing"),
            Page::NowPlaying,
        );
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
        components::card_panel(&theme)
            .rounded(px(design::radius::FULL))
            .flex()
            .items_center()
            .gap(px(spacing::SM))
            .px(px(spacing::SM))
            .child(menu)
            .child(
                components::content_button(&theme, "dashboard-quick-settings", variant(Page::QuickSettings))
                    .px(px(spacing::XL))
                    // 数字で幅が変わって落ち着かないので、時刻の幅を固定する。
                    .child(fixed_clock(time_width))
                .tooltip(i18n::t(cx, "dashboard.quick_settings"))
                .children(battery.map(|b| design::icon(battery_glyph(&b)).size(px(font_size::LG))))
                .on_click(cx.listener(|this, _, _, cx| this.toggle(Page::QuickSettings, cx))),
            )
            // 何か再生しているときだけ、再生中の画面のボタンを出す。
            .when(has_media, |d| d.child(now_playing))
            .child(mirror)
            .when(!streams.is_empty(), |d| d.child(separator(&theme)))
            // 開いた配信はここに溜まり、はみ出すと横にスクロールする。
            .child(
                div()
                    .id("dashboard-streams")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .children(streams),
            )
    }
}

/// クイック設定・画面のミラーリングの起動仕様（`page` はそのどちらか）。開くと `opened` でダッシュボードにはまる。
/// 閉じるのはナビゲーションバーかバーの同じボタン。
fn screen_spec(
    from: PanelId,
    page: Page,
    dashboard: WeakEntity<Dashboard>,
    opened: Box<dyn FnOnce(PanelId, &mut App)>,
) -> PanelSpec {
    let title = match page {
        Page::Mirror => "dashboard.mirror",
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
    /// 再生中の画面の画像（ジャケット・背景・アプリのアイコン）。元の画像が変わったときだけ作り直す。
    media_images: Option<MediaImages>,
    /// 取得元の一覧が届いたら、再生中のアプリのウィンドウを開く。
    open_app: bool,
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
        tick_every_second(panel, cx);
        let mut this = Self {
            panel,
            page,
            dashboard,
            sources: Vec::new(),
            pending: None,
            error: None,
            scroll: ScrollHandle::new(),
            media_images: None,
            open_app: false,
        };
        // 再生中の画面は、アプリのアイコンが OS から取れないとき（デスクトップのアプリ）にウィンドウのものを使う。
        if matches!(page, Page::Mirror | Page::NowPlaying) {
            this.refresh();
        }
        this
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
                })
                    || (!name.is_empty() && s.name.to_lowercase().contains(&name)))
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
            match window.map(|(source, icon)| (source, icon, mirror_of(source, cx).map(|(id, _)| id))) {
                // もう映していれば新しく開かない。ダッシュボードに隠している配信なら、それを出す。
                Some((_, _, Some(id))) => {
                    let _ = self.dashboard.update(cx, |d, cx| d.show_stream(id, cx));
                }
                Some((source, icon, None)) => {
                    let dashboard = (ui::docked(cx) == Some(self.panel)).then(|| self.dashboard.clone());
                    let spec = mirror_spec(source.clone(), icon.clone(), dashboard, self.panel, false);
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
                i.as_ref().and_then(|i| rgba_image(i.width, i.height, &i.rgba))
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
        let app_window = self.app_window(&m.app);
        let can_open_app = app_window.is_some() || m.controls.raise;
        let icon = images
            .icon
            .clone()
            .or_else(|| app_window.and_then(|(_, i)| i.clone()));
        let panel = self.panel;
        let send = move |command| move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            commands(panel, [PanelCommand::Media(command)], cx)
        };
        let control = |id: &'static str, glyph, key, enabled: bool, command| {
            components::icon_button(&theme, id, glyph, i18n::t(cx, key), Variant::Ghost)
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
                                        gpui::canvas(move |bounds, _, _| bounds_in.set(bounds), |_, _, _, _| {})
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
                                    .when_some(duration.filter(|_| m.controls.seek), |d, duration| {
                                        d.on_mouse_down(gpui::MouseButton::Left, move |e, _, cx| {
                                            let b = seek_bounds.get();
                                            let at = f32::from(e.position.x - b.origin.x)
                                                / f32::from(b.size.width).max(1.0);
                                            let to = duration.mul_f32(at.clamp(0.0, 1.0));
                                            commands(panel, [PanelCommand::Media(media::Command::Seek(to))], cx);
                                        })
                                    }),
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
                                if m.playing { &icons::PAUSE } else { &icons::PLAY },
                                if m.playing { "dashboard.pause" } else { "dashboard.play" },
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

    fn mirror(&mut self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        self.poll_sources();
        let (dashboard, panel) = (self.dashboard.clone(), self.panel);
        let tiles = |kind: SourceKind| {
            let tiles: Vec<_> = self
                .sources
                .iter()
                .enumerate()
                .filter(|(_, (s, _))| s.kind == kind)
                // 常駐の画面（起動時にダッシュボードへ置いたもの）は、いつもバーにあるので一覧に出さない。
                .filter(|(_, (s, _))| !mirror_of(s, cx).is_some_and(|(_, resident)| resident))
                .map(|(i, (source, image))| {
                    let glyph = match (kind, image) {
                        (_, Some(image)) => img(image.clone())
                            .size(px(TILE_ICON_PX))
                            .into_any_element(),
                        (SourceKind::Screen, None) => tile_icon(&icons::MONITOR),
                        (SourceKind::Window, None) => tile_icon(&icons::APP_WINDOW),
                    };
                    // もう映しているもの（同じ画面・ウィンドウは 1 枚だけ）は押せない。
                    let open = mirror_of(source, cx).is_some();
                    let (source, dashboard) = (source.clone(), dashboard.clone());
                    components::tile(theme, format!("dashboard-source-{i}"))
                        .h(px(TILE_PX))
                        .min_w_0()
                        .child(glyph)
                        .child(tile_label(source.name.clone()))
                        .when(open, |t| t.opacity(0.4))
                        .when(!open, |t| t.on_click({
                            let icon = image.clone();
                            move |_, _, cx| {
                                // 映したものは、この一覧と入れ替わりにダッシュボードにはまり、バーにボタンが増える。
                                // 一覧を独立させていれば、映したものも独立して開く。
                                let dashboard =
                                    (ui::docked(cx) == Some(panel)).then(|| dashboard.clone());
                                cx.default_global::<PanelControl>().spawns.push(mirror_spec(
                                    source.clone(),
                                    icon.clone(),
                                    dashboard,
                                    panel,
                                    false,
                                ));
                            }
                        }))
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

    fn quick_settings(&self, theme: &Theme, time_width: gpui::Pixels, cx: &mut Context<Self>) -> Div {
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
            Page::Mirror => self.mirror(&theme, cx),
            Page::NowPlaying => self.now_playing(cx),
            // デバッグ・設定は main が渡す別の画面で、Screen にはならない。
            _ => {
                let width = clock_width(window, QUICK_TIME_PX);
                self.quick_settings(&theme, width, cx)
            }
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
        .find(|(s, id, _)| s.same_target(source) && !panels.get(id).is_some_and(|p| p.status.closed))
        .map(|&(_, id, resident)| (id, resident))
}

/// `source` を映すパネルの起動仕様。`dashboard` があれば開くとそこにはまり（ミラーリングの一覧と入れ替わる）、
/// `None`（独立させた画面から開いた）なら独立して、開いた画面 `from` の手前に重ねて開く。
/// `resident` なら（`dashboard` があるとき）常駐のもの: はめずに隠して開き、バーのボタンで出す。ナビゲーションバーでは
/// 閉じずにしまう。
/// タイトルは取得元の名前で、ナビゲーションバーで閉じると配信も止まる。
fn mirror_spec(
    source: Source,
    icon: Option<Arc<RenderImage>>,
    dashboard: Option<WeakEntity<Dashboard>>,
    from: PanelId,
    resident: bool,
) -> PanelSpec {
    let (title, source_title, kind) = (source.name.clone(), source.name.clone(), source.kind);
    let docked = dashboard.is_some();
    let minimize = dashboard.clone().filter(|_| resident);
    let build = move |panel: PanelId, _: &mut Window, cx: &mut Context<Mirror>| {
        // 閉じたものは忘れ、これを覚える。
        let closed = |id: &PanelId| {
            cx.try_global::<PanelControl>()
                .and_then(|c| c.panels.get(id))
                .is_some_and(|p| p.status.closed)
        };
        let mut mirrors: Vec<_> = cx
            .try_global::<Mirrors>()
            .map(|m| m.0.iter().filter(|(_, id, _)| !closed(id)).cloned().collect())
            .unwrap_or_default();
        mirrors.push((source.clone(), panel, resident && dashboard.is_some()));
        cx.set_global(Mirrors(mirrors));
        cx.default_global::<PanelControl>()
            .panels
            .entry(panel)
            .or_default()
            .commands
            .push(PanelCommand::Stream(Some(source), DEFAULT_FRAME_RATE));
        if let Some(dashboard) = dashboard {
            if resident {
                commands(panel, [PanelCommand::Hide(true)], cx);
            } else {
                ui::dock(panel, cx);
            }
            let name = SharedString::from(title.clone());
            let _ = dashboard.update(cx, |d, cx| d.add_stream(panel, kind, name, icon, resident, cx));
        }
        Mirror { panel }
    };
    let spec = PanelSpec::spawn(from, build);
    // はめるものは、開いた元の手前ではなく、はめる場所に合わせて目の前に出す。
    let spec = if docked { spec.in_view(SCREEN_POSE) } else { spec };
    let spec = match minimize {
        Some(dashboard) => spec.minimizable(move |id, cx| {
            let _ = dashboard.update(cx, |d, cx| d.minimize(id, cx));
        }),
        None => spec,
    };
    // 配信は横幅を保って縦横比を合わせるので、幅をはめる画面とそろえる。
    spec.sized(SIZE_M, RES_PX)
        .titled_text(source_title)
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
    rgba_image(icon.width, icon.height, &icon.rgba)
}

/// RGBA の画素を GPUI の画像（BGRA）にする。
fn rgba_image(width: u32, height: u32, rgba: &[u8]) -> Option<Arc<RenderImage>> {
    let mut bgra = rgba.to_vec();
    for p in bgra.chunks_exact_mut(4) {
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
            window.text_system().shape_line(text, px(size), &[run], None).width
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

    fn summon(from: PanelId, opened: Box<dyn FnOnce(PanelId, &mut App)>) -> PanelSpec {
        PanelSpec::spawn(from, move |id, _, cx| {
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
        // 設定を選ぶと（メニューから。`menu_items_open_screens_and_close_the_menu`）、main が渡した設定画面がバーの上にはまる。
        bar.update(cx, |d, cx| d.toggle(Page::Settings, cx));
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
        assert!(spec.parent().is_some_and(|p| p.panel == 1 && p.docked && p.dismiss));
        let (_menu, mcx) = cx.add_window_view(|_, _| Menu {
            panel: 9,
            dashboard: bar.downgrade(),
        });
        mcx.run_until_parked();
        click(mcx, "menu-settings");
        assert!(commands(mcx, 9).contains(&PanelCommand::Close));
        assert_eq!(mcx.update(|_, cx| cx.global::<PanelControl>().spawns.len()), 1);
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
        assert!(matches!(last, Some(PanelCommand::Media(media::Command::Seek(d))) if d > Duration::from_secs(190)));
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
                let status = &mut cx.default_global::<PanelControl>().panels.entry(1).or_default().status;
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
        // 一覧は入れ替わりに閉じ、ダッシュボードは開いたまま。バーに配信のボタンが増える。
        assert!(commands(cx, 2).contains(&PanelCommand::Close));
        assert!(!commands(cx, 1).contains(&PanelCommand::Close));
        let streams = cx.update(|_, cx| bar.read(cx).streams.iter().map(|s| s.panel).collect::<Vec<_>>());
        assert_eq!(streams, [4]);
        assert_eq!(cx.update(|_, cx| ui::docked(cx)), Some(4));
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
        assert_eq!(five[1], PanelCommand::Hide(false));
        assert!(!commands(cx, 4).contains(&PanelCommand::Close), "隠すだけで閉じない");
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
        let streams = cx.update(|_, cx| bar.read(cx).streams.iter().map(|s| s.panel).collect::<Vec<_>>());
        assert_eq!(streams, [5]);
    }

    /// 常駐の配信は、独立させてもバーに残り、しまうと閉じずに隠れる。
    #[gpui::test]
    fn resident_streams_stay_on_the_bar_and_minimize(cx: &mut TestAppContext) {
        let (bar, cx) = cx.add_window_view(|_, cx| {
            cx.default_global::<PanelControl>().dock = Some((1, BAR_TILT));
            Dashboard::new(1, summon, summon, cx)
        });
        bar.update(cx, |d, cx| d.add_stream(4, SourceKind::Screen, "Display 1".into(), None, true, cx));
        // はめてから自分で外した（独立させた）。
        let set_parent = |cx: &mut VisualTestContext, parent| {
            cx.update(|_, cx| {
                cx.default_global::<PanelControl>().panels.entry(4).or_default().status.parent = parent
            });
            bar.update(cx, |d, cx| d.prune_streams(cx));
        };
        set_parent(cx, Some(1));
        set_parent(cx, None);
        assert_eq!(bar.read_with(cx, |d, _| d.streams.len()), 1, "独立させても残る");
        bar.update(cx, |d, cx| d.minimize(4, cx));
        let c = commands(cx, 4);
        assert!(c.contains(&PanelCommand::Hide(true)));
        assert!(!c.contains(&PanelCommand::Close), "閉じない");
        assert!(bar.read_with(cx, |d, _| d.streams[0].stashed));
    }
}
