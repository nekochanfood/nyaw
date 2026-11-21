//! Layer 6 画面: ダッシュボード。左手の X ボタンを二回連続で押すと開閉する
//! （常駐の wrist が [`kernel::PanelStatus::double_press`] を監視して制御する）。表示中は空間全体が暗転する。
//! ダッシュボードの実体はやや上向きに傾けたバー（[`Dashboard`]）であり、開いた直後には何も表示しない。バーのボタンで、
//! クイック設定・ランチャー・再生中（[`Screen`]）・デバッグ・設定の各画面（個別のナビゲーションバーを持つ別パネル）を
//! バーの上にドッキングする（`ui::dock`。同じボタンの再押下で閉じる）。ランチャーから起動したアプリのウィンドウも同様にドッキングされる。
//! ドッキングした画面はダッシュボードに追従し（バーを掴むとダッシュボードごと移動する）、掴んで引き離すかナビゲーションバーで
//! 取り外すと独立する。近づけて離すかナビゲーションバーで戻すと再びドッキングする。閉じる操作（バーの X・X ボタンの二回押し）では
//! ドッキング中の画面ごと非表示にするだけで、次回は同じ状態で復帰する。

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use design::components::slide::{Entering, Slide, fade_in, fit_bar, slide_in, with_leaving};
use design::components::{self, Variant};
use design::{Theme, font_size, icons, spacing};
use gpui::{
    AnyElement, App, Context, Div, FocusHandle, FontWeight, KeyDownEvent, ListAlignment, ListState,
    RenderImage, ScrollHandle, SharedString, Task, WeakEntity, Window, canvas, div, img, list,
    prelude::*, px, rgba,
};
use kernel::audio::Sound;
use kernel::desktop::{
    Icon, InstalledApp, InstalledApps, Source, SourceKind, Sources, Status, app_key,
};
use kernel::subsystems::media;
use kernel::subsystems::notifications::Notification;
use kernel::vr::{Pose, Quat, Vec2, Vec3};
use kernel::{PanelCommand, PanelId, Request};
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
use screen::*;

#[cfg(test)]
mod test_util;

/// バーの手前方向への傾斜角（見上げずに読めるよう上向きに傾ける）。ドッキングした画面はこの分だけ起こして垂直にする。
const BAR_TILT: f32 = std::f32::consts::FRAC_PI_6;
/// ドッキング画面のサイズと解像度（main が渡す設定画面と共通）。外部から渡される画面も縦をこの高さに統一する。
const SIZE_M: Vec2 = Vec2::new(1.024, 0.768);
const RES_PX: (u32, u32) = (1024, 768);
/// バーのウィンドウサイズ（px）。帯（ナビゲーションバーと同じ高さ）の周囲にシャドウ用の余白を含む。幅は画面と同密度。
const BAR_PX: (u32, u32) = (
    RES_PX.0,
    (components::NAV_BAR_HEIGHT + 2.0 * components::PANEL_MARGIN) as u32,
);
const BAR_M: Vec2 = Vec2::new(SIZE_M.x, SIZE_M.x * BAR_PX.1 as f32 / BAR_PX.0 as f32);
/// 高さ設定 0 の基準位置（頭の高さからの相対 m）。ドッキング画面の中心が目の高さよりやや下になる位置。
const HEIGHT_ORIGIN: f32 = -0.1;
/// 画面を開いた直後のドッキング位置（既定の距離・高さにおけるドッキング先。開くと即座にドッキングする）。
const SCREEN_POSE: Pose = Pose::new(
    Vec3::new(0.0, HEIGHT_ORIGIN, -kernel::DASHBOARD_DISTANCE),
    Quat::IDENTITY,
);
/// タイルの高さ（px）。
const TILE_PX: f32 = 120.0;
/// タイルのアイコンの大きさ（px）。
const TILE_ICON_PX: f32 = 48.0;
/// クイック設定の上の時刻の大きさ（px）。
const QUICK_TIME_PX: f32 = 48.0;
/// 開閉時の効果音。いずれもダッシュボード自体から再生される（閉じる音は閉じ方によらず再生する）。
pub const OPEN_SOUND: &[u8] = include_bytes!("../assets/dashboard_open.wav");
pub const CLOSE_SOUND: &[u8] = include_bytes!("../assets/dashboard_close.wav");

/// ダッシュボードから開く外部画面（デバッグ・設定）の起動仕様を生成する。第 1 引数は起動元のパネル
/// （[`PanelSpec::spawn`] に渡す）。起動後、コールバックで PanelId を通知する（[`spec`] と同じ形式）。
/// 画面間の直接結合を避けるため、起動対象は main が決定する。
pub type Summon = fn(PanelId, Opened) -> PanelSpec;
pub type Opened = Box<dyn FnOnce(PanelId, &mut App)>;

/// バーの配置姿勢（開いた瞬間の頭からの相対姿勢）。ドッキング画面（[`SIZE_M`]）の中心が正面 `distance`・頭の高さから
/// [`HEIGHT_ORIGIN`] + `height` 上に垂直に立つよう、画面の下半分・ナビゲーションバー・隙間の分だけ下げ、
/// 傾けたバーの上端がその直下に位置するよう配置する（オーバーレイ層の `Parent::slot` の逆算）。
pub fn pose(distance: f32, height: f32) -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    let face = SIZE_M.y / 2.0 - margin(SIZE_M, RES_PX);
    // ドッキング画面のナビゲーションバーは距離によらず固定サイズ（1m 先での表示サイズを維持）。
    let nav_bar = ui::NAV_BAR_HEIGHT_M;
    let top = Vec3::new(
        0.0,
        HEIGHT_ORIGIN + height - (face + nav_bar + kernel::overlay::SLOT_GAP),
        -distance,
    );
    let rotation = Quat::from_rotation_x(-BAR_TILT);
    let half = BAR_M.y / 2.0 - margin(BAR_M, BAR_PX);
    Pose::new(top - rotation * Vec3::Y * half, rotation)
}

/// 現在の設定値（距離・高さ）に基づくバーの配置姿勢（[`pose`]）。
fn placed(cx: &App) -> Pose {
    let s = ui::settings(cx);
    pose(s.dashboard_distance, s.dashboard_height)
}

/// ダッシュボード（バー）の起動仕様。`opened` は生成されたパネルの ID を受け取る。`hidden` が真の場合は非表示のまま生成する
/// （起動時に事前生成し、画面の配信を準備しておく。表示は [`show`] で行う）。
pub fn spec(
    opened: impl FnOnce(PanelId, &mut App) + 'static,
    settings: Summon,
    debug: Summon,
    hidden: bool,
) -> PanelSpec {
    let default = pose(kernel::DASHBOARD_DISTANCE, 0.0);
    PanelSpec::root(Pose::IDENTITY, move |id, _, cx: &mut Context<Dashboard>| {
        opened(id, cx);
        // 非表示で開く場合は開始音を再生しない（配置は表示時に行う）。即座に表示する場合は設定の距離・高さに再配置する。
        let entry = if hidden {
            vec![PanelCommand::Hide(true)]
        } else {
            let open = PanelCommand::Play(Sound::Wav(OPEN_SOUND), false);
            vec![PanelCommand::Place(placed(cx)), open]
        };
        let close_sound = PanelCommand::CloseSound(Sound::Wav(CLOSE_SOUND));
        commands(id, entry.into_iter().chain([close_sound]), cx);
        // バーの上をドッキングスロットとして登録する（掴んで引き離す・近づけてドッキングする操作にも対応）。
        ui::offer_dock(id, BAR_TILT, cx);
        Dashboard::new(id, settings, debug, cx)
    })
    .sized(BAR_M, BAR_PX)
    .in_view(default)
    // 閉じる（非表示にする）操作は X ボタンの二回押しかダッシュボード外の押下であり、閉じるボタンは設置しない。
    .without_nav_bar()
    // サイズはドッキング画面に合わせて固定のためリサイズ不可。
    .without_resize()
    // 画面がドッキングしていなくても、画面が立つ位置を基準に掴んで移動する（ドッキング画面を掴んだときと同じ挙動。距離・高さに依存しない）。
    .grab_anchor(default.inverse() * SCREEN_POSE)
    .with_veil()
}

/// ダッシュボードをドッキング画面ごと非表示にする（クローズではない。配信も停止しない）。
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

/// 非表示のダッシュボードを現在の正面に同じ状態で再表示する。ドッキング中の配信があれば、そのウィンドウを OS 上で前面化する。
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

/// ダッシュボードにドッキングする画面の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    QuickSettings,
    /// メディア再生中画面（[`Screen`]）。
    NowPlaying,
    /// ランチャー: アプリを検索・起動し、そのウィンドウを配信する（[`Screen`]）。
    Launcher,
    Debug,
    Settings,
}

/// ダッシュボード（バー）。左からメニュー（デバッグ・設定のポップオーバー）・時刻と電池（クイック設定）・
/// ランチャー、区切り線を挟んで開いた配信のボタン（蓄積され、あふれると横スクロールする）。
/// 閉じる（非表示にする）操作は X ボタンの二回押しかダッシュボード外の押下。
/// 現在ドッキング中の画面・配信のボタンは強調色で表示する。
pub struct Dashboard {
    panel: PanelId,
    settings: Summon,
    debug: Summon,
    /// ダッシュボードで開いた配信（画面・ウィンドウ）。押下でドッキングし、他を選択すると閉じずに非表示になる。
    streams: Vec<Stream>,
    /// 開いているメニューのポップオーバー（[`Menu`]）。
    menu: Option<PanelId>,
    /// 開いた画面（画面種別ごとに 1 つ。取り外して独立させたものも含む。閉じたものは [`Self::page_panel`] で除外される）。
    pages: Vec<(Page, PanelId)>,
    /// 反映した、何も無い所を押した回数（[`kernel::PanelStatus::outside_press`]）と、そのとき隠していたか。
    /// 最初に見た値から数える。
    outside: Option<(u32, bool)>,
    /// 最後に見た、設定のダッシュボードの距離・高さ（変わったら置き直す。[`Self::follow_placement`]）。
    placement: Option<(f32, f32)>,
    /// 足されたボタンの出方（[`Entering`]）。
    enter: Entering,
    /// 前のフレームに並べていたもの（消えていくものを含む）。消えたものを、滑り出る間だけ同じ場所に描き残すのに使う。
    prev_streams: Vec<Stream>,
    prev_media: bool,
    prev_update: bool,
    prev_sep: bool,
    /// バーの横スクロール（最大幅を超えたとき）。
    scroll: ScrollHandle,
    /// 応えた、通知センターを開く頼みの回数（[`ui::notification::Center::asked`]）。
    center_asked: u32,
    /// 通知センターを頼まれたときダッシュボードが隠れていた。出てからはめる（隠れている間ははめられない）。
    center_waiting: bool,
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
    /// 通知センター（クイック設定の一覧）で、ホバーして広げている通知・トーストから開いて広げて見せる通知（ホバーすると
    /// 外す）と、アプリのアイコン。
    notification_hover: Option<u32>,
    notification_focus: Option<u32>,
    notification_icons: ui::notification::Icons,
    /// スワイプで片付けた通知（OS の通知センターから消えて一覧から落ちるまで並べない）と、左右のスワイプ。
    dismissed: HashSet<u32>,
    swipes: ui::swipe::Swipes,
    /// 通知のカードを広げる・たたむ動き。
    expanding: ui::notification::Expanding,
    /// 「（アプリ）を開く」を押したアプリ（ウィンドウの一覧を取り直してから、映して開く）。
    open_after: Option<InstalledApp>,
}
