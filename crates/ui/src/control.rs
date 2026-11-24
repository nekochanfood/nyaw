//! ビューとフレームループ間の受け渡し口（[`PanelControl`]）および、ビューから利用する操作（パネルへの要求・全体設定・ドック）。

use super::*;

/// パネル単位の操作要求キューと状態。
#[derive(Debug, Default)]
pub struct PanelState {
    pub commands: Vec<PanelCommand>,
    pub status: PanelStatus,
    /// 表示中は毎フレーム再描画する。連続アニメーションを行うビューが設定する。
    pub live: bool,
    /// 次フレームで 1 回再描画する。タイマー等で自発的に更新するビューが設定し、フレームループが消費する。
    /// アイドル時の推測再描画（[`IDLE_REDRAW`]）を待たず、変更時にのみ再描画できる。
    pub redraw: bool,
    /// このパネルをレーザーで指している手の一覧。フレームループが [`sync_panel`] で毎フレーム更新する（変更時の再描画は不要）。
    /// コントローラ入力のフィルタ条件として使用（[`PanelControl::controllers`] の絞り込み）。
    pub pointing: Vec<Hand>,
    /// 取り付け先から見た相対姿勢（取り付け時のみ有効、掴んで離すと更新）と VR 上の大きさ（m、UI スケール除算済み）。
    /// フレームループが [`sync_panel`] で毎フレーム更新する（変更時の再描画は不要）。wrist が配置記憶に使用する。
    pub offset: Option<Pose>,
    pub size: Vec2,
    /// 最後の再描画時点の全体設定（[`PanelControl::settings`]）。変更があれば再描画する。
    seen: Settings,
}

/// ビューとフレームループ間の受け渡し口（GPUI グローバル）。ビューが `commands` にキューイングした操作を、
/// フレームループが [`sync_panel`] でオーバーレイに反映し `status` を書き戻す。
/// ビューの型に依存しないため、本番・testkit 共通で利用する。
#[derive(Default)]
pub struct PanelControl {
    pub panels: HashMap<PanelId, PanelState>,
    /// アプリ全体の設定（テーマ・言語・ポインタ）。フレームループが [`sync_settings`] で更新する。
    pub settings: Settings,
    /// ビューがキューイングする全体設定の変更要求（パネル非依存）。フレームループが [`sync_settings`] で反映する。
    pub setting_changes: Vec<Setting>,
    /// ビューがキューイングするシステムへの要求（メディア・OS の通知の操作。パネル非依存）。フレームループが
    /// [`sync_settings`] でカーネルへ渡す（[`Kernel::request`]）。
    pub requests: Vec<Request>,
    /// OS のフォアグラウンドアプリのキーボード状態（[`keyboard::system_keyboard`] で参照）。
    /// `os_keyboard_wanted` が設定された後、フレームループが [`sync_settings`] でカーネルから転写する。
    pub os_keyboard: kernel::subsystems::os::keyboard::Settings,
    pub(crate) os_keyboard_wanted: bool,
    pub spawns: Vec<PanelSpec>,
    /// プロファイリングの有効・無効。ビューが切り替え、フレームループが計測とプロファイラパネルの開閉を同期する。
    /// パネルの閉じるボタンからも無効化される。
    pub profiling: bool,
    /// 手ごとのタッチ長押し進捗（0〜1、[`PointerBridge::hold_progress`]）。ポインタのリング表示に使用する。
    pub hold: HashMap<Hand, f32>,
    /// 手ごとのツールチップ文字列（ポイント先の要素が [`design::components::Tooltips`] に設定した内容）と、
    /// カーソルに対する表示位置（上下）。フレームループが [`sync_tooltips`] で書き込み、ツールチップパネルが描画する。
    pub tooltip: HashMap<Hand, gpui::SharedString>,
    pub anchor: HashMap<Hand, Anchor>,
    /// スケルタル入力が取得できている手と HMD の姿勢。フレームループが [`sync_panel`] で毎フレーム更新する。
    /// パネル固有の状態ではないため、変更時に再描画は行わない（必要なビューは `PanelState::live` で毎フレーム描画する）。
    pub hands: Vec<kernel::vr::HandSkeleton>,
    pub hmd: kernel::Pose,
    /// 手ごとの入力状態（判定の確認用）。`hands` と同様に毎フレーム更新する。
    pub controllers: Vec<kernel::ControllerState>,
    /// バッテリー残量を報告しているデバイス。`hands` と同様に毎フレーム更新する（変更時の再描画は不要）。
    pub batteries: Vec<kernel::vr::Battery>,
    /// 再生中のメディア（[`kernel::Kernel::now_playing`]）。`hands` と同様に毎フレーム更新する。
    pub media: Option<std::sync::Arc<kernel::subsystems::media::NowPlaying>>,
    /// 起動後に受信した OS 通知（[`kernel::Kernel::notifications`]、受信順）。`hands` と同様に毎フレーム更新する。
    pub notifications:
        std::sync::Arc<[std::sync::Arc<kernel::subsystems::notifications::Notification>]>,
    /// 両目の視野範囲（[`kernel::Kernel::fov`]）。HUD は `Fov::safe` の内側に配置する。`hands` と同様に毎フレーム更新する。
    pub fov: Option<kernel::Fov>,
    /// アプリの終了要求。フレームループがアプリを終了する（テストハーネスも同様）。
    pub quit: bool,
    /// 開いているパネル数（閉じたものを除く）と同時オープン上限（[`MAX_PANELS`] とランタイム上限の小さい方）。
    /// フレームループが毎フレーム更新する。
    pub open: usize,
    pub limit: usize,
    /// 持ち主パネルからナビゲーションバーへの対応。バーの起動時に登録する（持ち主の状態変化でバーを再描画するため）。
    pub nav_bars: HashMap<PanelId, PanelId>,
    /// ドック（ダッシュボード）: 持ち主パネルと、上端に立てる角度（[`kernel::Parent::slot`]）。[`dock`] で使用する。
    pub dock: Option<(PanelId, f32)>,
    /// 現在ドックに装着されている画面（[`docked`] で有効性を検証してから参照する）。
    docked: Option<PanelId>,
}

/// ドック（[`PanelControl::dock`]）が有効（閉じられても隠されてもいない）であれば、その持ち主と角度を返す。
pub(crate) fn dock_slot(cx: &App) -> Option<(PanelId, f32)> {
    let ctl = cx.try_global::<PanelControl>()?;
    ctl.dock.filter(|(owner, _)| {
        !ctl.panels
            .get(owner)
            .is_some_and(|p| p.status.closed || p.status.hidden)
    })
}

/// 現在ドックに装着されている画面。
pub fn docked(cx: &App) -> Option<PanelId> {
    let (owner, _) = dock_slot(cx)?;
    let ctl = cx.global::<PanelControl>();
    ctl.docked.filter(|id| {
        ctl.panels
            .get(id)
            .is_none_or(|p| !p.status.closed && p.status.parent.is_none_or(|p| p == owner))
    })
}

/// `panel` をドックに装着する（持ち主に追従し、掴むと持ち主ごと移動する）。既存の装着画面は閉じる。
/// ドックが利用不可の場合は何もしない。
pub fn dock(panel: PanelId, cx: &mut App) {
    let Some((owner, tilt)) = dock_slot(cx) else {
        return;
    };
    if let Some(previous) = docked(cx).filter(|&p| p != panel) {
        command(previous, PanelCommand::Close, cx);
    }
    cx.default_global::<PanelControl>().docked = Some(panel);
    let parent = kernel::Parent {
        panel: owner,
        carry: true,
        docked: true,
        slot: Some(tilt),
        ..Default::default()
    };
    command(panel, PanelCommand::Parent(Some(parent)), cx);
}

/// `panel` をドックから取り外して独立させる（その場に残り、持ち主が閉じても存続する）。
pub fn undock(panel: PanelId, cx: &mut App) {
    let ctl = cx.default_global::<PanelControl>();
    if ctl.docked == Some(panel) {
        ctl.docked = None;
    }
    command(panel, PanelCommand::Parent(None), cx);
}

/// `owner` の上端をドックとして登録する（`tilt` 分の角度で起こして立てる。[`dock`] で使用）。
/// ドック対応画面（[`PanelSpec::dockable`]）は掴んで引き抜き、近づけて離すと装着される（オーバーレイ層のマグネット）。
pub fn offer_dock(owner: PanelId, tilt: f32, cx: &mut App) {
    cx.default_global::<PanelControl>().dock = Some((owner, tilt));
    command(owner, PanelCommand::DockSlot(Some(tilt)), cx);
}

/// ドックへの装着音と取り外し音。
pub(crate) const DOCK_SOUND: &[u8] = include_bytes!("../assets/dock/dock.wav");
pub(crate) const UNDOCK_SOUND: &[u8] = include_bytes!("../assets/dock/undock.wav");

/// 着脱した `panel` から、装着音（`docked` が真）か取り外し音を再生する。
/// 手やメニューで着脱したときに鳴らす。[`dock`]・[`undock`] 自体は鳴らさないので、バーのボタンによる画面の切り替えは無音になる。
pub(crate) fn dock_sound(panel: PanelId, docked: bool, cx: &mut App) {
    let wav = if docked { DOCK_SOUND } else { UNDOCK_SOUND };
    let sound = kernel::audio::Sound::Wav(wav);
    command(panel, PanelCommand::Play(sound, false), cx);
}

/// ドックの着脱変更（[`Kernel::take_dock_changes`]）を装着記録に反映し、ドック持ち主の
/// ボタン強調状態と該当画面のナビゲーションバー（取り外し・復帰ボタン）を再描画させる。着脱音も鳴らす。
fn sync_docking<B: VrBackend>(kernel: &mut Kernel<B>, cx: &mut App) {
    let changes = kernel.take_dock_changes();
    for &(panel, docked) in &changes {
        let ctl = cx.default_global::<PanelControl>();
        if docked {
            ctl.docked = Some(panel);
        } else if ctl.docked == Some(panel) {
            ctl.docked = None;
        }
        let owner = ctl.dock.map(|(o, _)| o);
        let bar = ctl.nav_bars.get(&panel).copied();
        for p in owner.into_iter().chain(bar) {
            redraw(p, cx);
        }
        dock_sound(panel, docked, cx);
    }
    if !changes.is_empty() {
        cx.refresh_windows();
    }
}

impl gpui::Global for PanelControl {}

/// `panel` への操作をキューイングする（フレームループが次の [`sync_panel`] でカーネルに反映する）。
pub fn command(panel: PanelId, command: PanelCommand, cx: &mut App) {
    commands(panel, [command], cx);
}

/// [`command`] の複数版。キューイングされた順に反映する。
pub fn commands(panel: PanelId, commands: impl IntoIterator<Item = PanelCommand>, cx: &mut App) {
    let state = cx.default_global::<PanelControl>().panels.entry(panel);
    state.or_default().commands.extend(commands);
}

/// `panel` の次フレームでの 1 回再描画を要求する（[`PanelState::redraw`]）。
pub fn redraw(panel: PanelId, cx: &mut App) {
    let state = cx.default_global::<PanelControl>().panels.entry(panel);
    state.or_default().redraw = true;
}

/// 自発的に更新したビュー（タイマー・非同期処理の結果）からの `cx.notify()`。
/// フレームループはビューの notify を検知できないため、[`redraw`] も併せて要求する。
pub fn notify<V: 'static>(panel: PanelId, cx: &mut Context<V>) {
    cx.notify();
    redraw(panel, cx);
}

/// 全体設定の変更をキューイングする（フレームループが次の [`sync_settings`] で反映する）。
pub fn set(setting: Setting, cx: &mut App) {
    let control = cx.default_global::<PanelControl>();
    control.setting_changes.push(setting);
}

/// システムへの要求をキューイングする（フレームループが次の [`sync_settings`] でカーネルへ渡す）。
pub fn request(request: Request, cx: &mut App) {
    cx.default_global::<PanelControl>().requests.push(request);
}

/// 現在の全体設定（フレームループが書き戻した値、未設定時は既定値）。
pub fn settings(cx: &App) -> Settings {
    let control = cx.try_global::<PanelControl>();
    control.map(|c| c.settings).unwrap_or_default()
}

/// ビューがキューイングした操作要求を `panel` に反映し、状態を書き戻す。変更があればウィンドウを再描画する（`true` を返す）。
/// 全体設定（[`sync_settings`]）とドック着脱（[`sync_docking`]）も内部で反映するため、呼び出し側はパネルごとにこの関数のみ
/// 呼べばよい。入力配送後・描画前に毎フレーム呼び出す。
pub fn sync_panel<B: VrBackend>(kernel: &mut Kernel<B>, panel: PanelId, cx: &mut App) -> bool {
    sync_settings(kernel, cx);
    sync_docking(kernel, cx);
    let ctl = cx.default_global::<PanelControl>();
    ctl.hands = kernel.hands().to_vec();
    ctl.hmd = kernel.hmd();
    ctl.controllers = kernel.controllers().to_vec();
    ctl.batteries = kernel.batteries().to_vec();
    ctl.media = kernel.now_playing();
    ctl.notifications = kernel.notifications();
    ctl.fov = kernel.fov();
    let settings = ctl.settings;
    let ctl = ctl.panels.entry(panel).or_default();
    ctl.pointing = kernel.pointing_hands(panel);
    for c in ctl.commands.drain(..) {
        kernel.apply(panel, c);
    }
    let p = kernel.panel(panel);
    ctl.offset = p.attached.as_ref().map(|(_, offset)| *offset);
    ctl.size = p.size / kernel.ui_scale();
    let status = kernel.status(panel);
    if ctl.status == status && ctl.seen == settings {
        return false;
    }
    ctl.status = status;
    ctl.seen = settings;
    if ctl.status.closed {
        keyboard::panel_closed(panel, cx);
    } else if ctl.status.hidden {
        keyboard::panel_hidden(panel, cx);
    }
    cx.refresh_windows();
    true
}

/// ビューがキューイングした全体設定の変更とシステムへの要求をカーネルに反映し、現在の設定を書き戻す。設定に変更があれば `true`。
/// テーマ・言語は GPUI グローバルにも設定する（画面は `design::Theme::current` / `i18n::t` で参照する）。
pub fn sync_settings<B: VrBackend>(kernel: &mut Kernel<B>, cx: &mut App) -> bool {
    let ctl = cx.default_global::<PanelControl>();
    for s in ctl.setting_changes.drain(..) {
        kernel.set(s);
    }
    for r in ctl.requests.drain(..) {
        kernel.request(r);
    }
    if ctl.os_keyboard_wanted {
        ctl.os_keyboard = kernel.os_keyboard();
    }
    let settings = kernel.settings();
    let changed = ctl.settings != settings;
    ctl.settings = settings;
    // 文字サイズと太字化は GPUI 側（スレッド単位）に保持する。不整合があれば、設定未変更でも同期する。
    design::font::set_text(settings.text_scale, settings.bold_text, cx);
    if !changed {
        return false;
    }
    design::Theme::set_mode(
        match settings.theme {
            ThemeMode::Dark => design::Mode::Dark,
            ThemeMode::Light => design::Mode::Light,
        },
        cx,
    );
    let accent = settings
        .personal_color
        .map(|c| design::Accent::new(c.lightness, c.chroma, c.hue));
    design::Theme::set_accent(accent.unwrap_or_default(), cx);
    cx.set_global(lang(settings.language));
    true
}

/// カーネルの言語設定を i18n の言語に変換する。
pub fn lang(l: Language) -> i18n::Lang {
    match l {
        Language::En => i18n::Lang::En,
        Language::Ja => i18n::Lang::Ja,
    }
}

/// i18n の言語をカーネルの言語設定に変換する（[`lang`] の逆変換）。
pub fn language(l: i18n::Lang) -> Language {
    match l {
        i18n::Lang::En => Language::En,
        i18n::Lang::Ja => Language::Ja,
    }
}

#[cfg(test)]
mod tests;
