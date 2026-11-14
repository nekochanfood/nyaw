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
    Icon, InstalledApp, InstalledApps, Source, SourceKind, Sources, Status, app_key,
};
use kernel::subsystems::media;
use kernel::vr::{Battery, Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId, Setting};
use ui::mirror::{MirrorState, mirror_screen};
use ui::{PanelControl, PanelSpec, commands};

mod bar;
mod launcher;
mod menu;
mod mirror;
mod now_playing;
mod parts;
mod quick_settings;
mod screen;

use bar::*;
use launcher::*;
pub use menu::*;
use mirror::*;
use now_playing::*;
use parts::*;
use quick_settings::*;
use screen::*;

#[cfg(test)]
mod test_util;

/// バーを手前に傾ける角度（下から見上げずに読めるよう、少し上を向かせる）。はめた画面はこの分だけ起こして垂直に立てる。
const BAR_TILT: f32 = std::f32::consts::FRAC_PI_6;
/// はめる画面の大きさと解像度（main が渡す設定画面と同じ）。ほかから渡される画面も、縦をこの高さに揃える。
const SIZE_M: Vec2 = Vec2::new(1.024, 0.768);
const RES_PX: (u32, u32) = (1024, 768);
/// バーの窓（px）。帯（ナビゲーションバーと同じ高さ）の周りに影の余白を取る。幅は画面と同じ密度。
const BAR_PX: (u32, u32) = (
    RES_PX.0,
    (components::NAV_BAR_HEIGHT + 2.0 * components::PANEL_MARGIN) as u32,
);
const BAR_M: Vec2 = Vec2::new(SIZE_M.x, SIZE_M.x * BAR_PX.1 as f32 / BAR_PX.0 as f32);
/// 画面を開いてからはまるまでの、最初の置き場所（既定の距離・高さでのはまる所。開くとすぐはまる）。
const SCREEN_POSE: Pose = Pose::new(
    Vec3::new(0.0, 0.0, -kernel::DASHBOARD_DISTANCE),
    Quat::IDENTITY,
);
/// タイルの高さ（px）。
const TILE_PX: f32 = 120.0;
/// タイルのアイコンの大きさ（px）。
const TILE_ICON_PX: f32 = 48.0;
/// クイック設定の上の時刻の大きさ（px）。
const QUICK_TIME_PX: f32 = 48.0;
/// 開くとき・閉じるときの音。どちらもダッシュボード自身から聞こえる（閉じる音は閉じ方を問わず鳴る）。
pub const OPEN_SOUND: &[u8] = include_bytes!("../assets/dashboard_open.wav");
pub const CLOSE_SOUND: &[u8] = include_bytes!("../assets/dashboard_close.wav");

/// ダッシュボードから開くほかの画面（デバッグ・設定）の起動仕様を作る。第 1 引数は開いた元のパネル
/// （[`PanelSpec::spawn`] に渡す）。開いたら引数で PanelId を知らせる（[`spec`] と同じ形）。
/// 画面どうしを直接つながないよう、何を開くかは main が選ぶ。
pub type Summon = fn(PanelId, Opened) -> PanelSpec;
pub type Opened = Box<dyn FnOnce(PanelId, &mut App)>;

/// バーの置き場所（開いた瞬間の頭から見た相対姿勢）。はめた画面（[`SIZE_M`]）の中心が正面 `distance`・頭の高さから `height` 上に
/// 垂直に立つよう（設定のダッシュボードの距離・高さ）、画面の下半分・そのナビゲーションバー・隙間の分だけ下げ、傾けたバーの
/// 上端がその真下に来るように置く（オーバーレイ層の `Parent::slot` の置き方の逆）。
pub fn pose(distance: f32, height: f32) -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    let face = SIZE_M.y / 2.0 - margin(SIZE_M, RES_PX);
    // はめた画面のナビゲーションバーは、距離で大きさを変えない（1m 先での大きさのまま）。
    let nav_bar = ui::NAV_BAR_HEIGHT_M;
    let top = Vec3::new(
        0.0,
        height - (face + nav_bar + kernel::overlay::SLOT_GAP),
        -distance,
    );
    let rotation = Quat::from_rotation_x(-BAR_TILT);
    let half = BAR_M.y / 2.0 - margin(BAR_M, BAR_PX);
    Pose::new(top - rotation * Vec3::Y * half, rotation)
}

/// 今の設定のダッシュボードの距離・高さでの、バーの置き場所（[`pose`]）。
fn placed(cx: &App) -> Pose {
    let s = ui::settings(cx);
    pose(s.dashboard_distance, s.dashboard_height)
}

/// ダッシュボード（バー）の起動仕様。`opened` は作られたパネルの ID を受け取る。`hidden` なら隠したまま開く
/// （起動時に作っておき、画面の配信を用意しておく。出すのは [`show`]）。
pub fn spec(
    opened: impl FnOnce(PanelId, &mut App) + 'static,
    settings: Summon,
    debug: Summon,
    hidden: bool,
) -> PanelSpec {
    let default = pose(kernel::DASHBOARD_DISTANCE, 0.0);
    PanelSpec::root(Pose::IDENTITY, move |id, _, cx: &mut Context<Dashboard>| {
        nyapp::opened(nyapp::system::DASHBOARD.id, id, cx);
        opened(id, cx);
        // 隠して開くときは開く音を鳴らさない（置くのは出すとき）。すぐ出すときは設定の距離・高さに置き直す。
        let entry = if hidden {
            vec![PanelCommand::Hide(true)]
        } else {
            let open = PanelCommand::Play(Sound::Wav(OPEN_SOUND), false);
            vec![PanelCommand::Place(placed(cx)), open]
        };
        let close_sound = PanelCommand::CloseSound(Sound::Wav(CLOSE_SOUND));
        commands(id, entry.into_iter().chain([close_sound]), cx);
        // バーの上を、画面をはめる場所にする。
        cx.default_global::<PanelControl>().dock = Some((id, BAR_TILT));
        Dashboard::new(id, settings, debug, cx)
    })
    .sized(BAR_M, BAR_PX)
    .in_view(default)
    // 閉じる（隠す）のは X ボタンの二回押しかダッシュボードの外を押したときで、閉じるボタンは置かない。
    .without_nav_bar()
    // 大きさははめる画面に合わせて決めてあるので変えさせない。
    .without_resize()
    // 画面がはまっていなくても、画面が立つ所を基準に掴んで動かす（はめた画面を掴んだときと同じ動き。距離・高さによらない）。
    .grab_anchor(default.inverse() * SCREEN_POSE)
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
            PanelCommand::Place(placed(cx)),
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
    /// 最後に見た、設定のダッシュボードの距離・高さ（変わったら置き直す。[`Self::follow_placement`]）。
    placement: Option<(f32, f32)>,
    /// 足されたボタンの出方（[`Entering`]）。
    enter: Entering,
    /// 前のフレームに並べていたもの（消えていくものを含む）。消えたものを、滑り出る間だけ同じ場所に描き残すのに使う。
    prev_streams: Vec<Stream>,
    prev_activities: Vec<nyapp::activity::Live>,
    prev_media: bool,
    prev_update: bool,
    prev_sep: bool,
    /// バーの横スクロール（最大幅を超えたとき）。
    scroll: ScrollHandle,
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
