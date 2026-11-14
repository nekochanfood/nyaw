//! ビューとフレームループの受け渡し口（[`PanelControl`]）と、ビューが使う操作（パネルへの要求・全体の設定・はめる場所）。

use super::*;

/// 1パネル分の操作要求と状態。
#[derive(Debug, Default)]
pub struct PanelState {
    pub commands: Vec<PanelCommand>,
    pub status: PanelStatus,
    /// 見えている間は毎フレーム描き直す（アニメーションし続けるビューが立てる）。
    pub live: bool,
    /// 次のフレームで 1 回描き直す（タイマーなどで自分から変わるビューが立て、フレームループが下ろす）。
    /// 何もしていないパネルの推測の描き直し（[`IDLE_REDRAW`]）を待たずに、変わったときだけ描ける。
    pub redraw: bool,
    /// このパネルそのものをレーザーで指している手。フレームループが [`sync_panel`] で毎フレーム書く（変わっても描き直させない）。
    /// コントローラの入力をパネルに見せてよいかの条件（[`PanelControl::controllers`] を絞る）。
    pub pointing: Vec<Hand>,
    /// このパネルが最後に描き直したときの全体の設定（[`PanelControl::settings`]。変われば描き直す）。
    seen: Settings,
}

/// ビューとフレームループの受け渡し口（GPUI のグローバル）。ビューは対象パネルの `commands` に積み、
/// フレームループが [`sync_panel`] でオーバーレイに反映して `status` を書き戻す。
/// ビューの型に依存しないので、本番・testkit のどのキットも同じ経路を通る。
#[derive(Default)]
pub struct PanelControl {
    pub panels: HashMap<PanelId, PanelState>,
    /// アプリ全体の設定（テーマ・言語・ポインタ）。フレームループが [`sync_settings`] で書き戻す。
    pub settings: Settings,
    /// ビューが積む、全体の設定の変更要求（パネルを問わない）。フレームループが [`sync_settings`] で反映する。
    pub setting_changes: Vec<Setting>,
    /// OS の前面のアプリのキーボードの状態（[`keyboard::system_keyboard`] で読む）。
    /// 読まれてから（`os_keyboard_wanted`）フレームループが [`sync_settings`] でカーネルから書き写す。
    pub os_keyboard: kernel::subsystems::os::keyboard::Settings,
    pub(crate) os_keyboard_wanted: bool,
    pub spawns: Vec<PanelSpec>,
    /// 計測するか。ビューが切り替え、フレームループが計測とプロファイラパネルの開閉を合わせる。
    /// パネルの閉じるボタンでも `false` になる。
    pub profiling: bool,
    /// 手ごとの、タッチの長押しの進み具合（0〜1。[`PointerBridge::hold_progress`]）。ポインタ（カーソル）がリングで見せる。
    pub hold: HashMap<Hand, f32>,
    /// 手ごとの、ツールチップの文字（指している要素が [`design::components::Tooltips`] に置いたもの）と、
    /// カーソルの上下どちらに出すか。フレームループが [`sync_tooltips`] で書き、ツールチップのパネルが描く。
    pub tooltip: HashMap<Hand, gpui::SharedString>,
    pub anchor: HashMap<Hand, Anchor>,
    /// 骨格が取れている手（スケルタル入力）と HMD の姿勢。フレームループが [`sync_panel`] で毎フレーム書く。
    /// どのパネルの状態でもないので、変わってもパネルを描き直させない（見たいビューは `PanelState::live` で毎フレーム描く）。
    pub hands: Vec<kernel::vr::HandSkeleton>,
    pub hmd: kernel::Pose,
    /// 手ごとの入力の状態（判定の確認用）。`hands` と同じく毎フレーム書く。
    pub controllers: Vec<kernel::ControllerState>,
    /// 電池の残量を申告しているデバイス。`hands` と同じく毎フレーム書く（変わってもパネルを描き直させない）。
    pub batteries: Vec<kernel::vr::Battery>,
    /// 再生中のメディア（[`kernel::Kernel::now_playing`]）。`hands` と同じく毎フレーム書く。
    pub media: Option<std::sync::Arc<kernel::subsystems::media::NowPlaying>>,
    /// 終了を頼む。フレームループがアプリを終える（テストのハーネスもこれで終える）。
    pub quit: bool,
    /// 開いているパネルの数（閉じたものは数えない）と、同時に開ける数の上限（[`MAX_PANELS`] とランタイムの上限の小さい方）。
    /// フレームループが毎フレーム書く。
    pub open: usize,
    pub limit: usize,
    /// 持ち主のパネル → そのナビゲーションバー。バーが開いたときに書く（持ち主の状態でバーを描き直すため）。
    pub nav_bars: HashMap<PanelId, PanelId>,
    /// 画面をはめられる場所（ダッシュボード）: 持ち主と、その上端に立てる角度（[`kernel::Parent::slot`]）。[`dock`] で使う。
    pub dock: Option<(PanelId, f32)>,
    /// 今はめている画面（[`docked`] で、まだはまっているか確かめて読む）。
    docked: Option<PanelId>,
}

/// 画面をはめる場所（[`PanelControl::dock`]）が出ていれば（閉じても隠してもいなければ）、その持ち主と角度。
pub(crate) fn dock_slot(cx: &App) -> Option<(PanelId, f32)> {
    let ctl = cx.try_global::<PanelControl>()?;
    ctl.dock.filter(|(owner, _)| {
        !ctl.panels
            .get(owner)
            .is_some_and(|p| p.status.closed || p.status.hidden)
    })
}

/// はめる場所に今はまっている画面。
pub fn docked(cx: &App) -> Option<PanelId> {
    let (owner, _) = dock_slot(cx)?;
    let ctl = cx.global::<PanelControl>();
    ctl.docked.filter(|id| {
        ctl.panels
            .get(id)
            .is_none_or(|p| !p.status.closed && p.status.parent.is_none_or(|p| p == owner))
    })
}

/// `panel` をはめる場所にはめる（持ち主に付いて動き、掴むと持ち主ごと動く）。はまっていた画面は閉じる。
/// 場所が開いていなければ何もしない。
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

/// はめた `panel` を外して独立させる（その場に残り、持ち主が閉じても閉じない）。
pub fn undock(panel: PanelId, cx: &mut App) {
    let ctl = cx.default_global::<PanelControl>();
    if ctl.docked == Some(panel) {
        ctl.docked = None;
    }
    command(panel, PanelCommand::Parent(None), cx);
}

impl gpui::Global for PanelControl {}

/// `panel` への操作を積む（フレームループが次の [`sync_panel`] でカーネルに反映する）。
pub fn command(panel: PanelId, command: PanelCommand, cx: &mut App) {
    commands(panel, [command], cx);
}

/// [`command`] の複数版（積んだ順に反映する）。
pub fn commands(panel: PanelId, commands: impl IntoIterator<Item = PanelCommand>, cx: &mut App) {
    let state = cx.default_global::<PanelControl>().panels.entry(panel);
    state.or_default().commands.extend(commands);
}

/// `panel` を次のフレームで 1 回描き直させる（[`PanelState::redraw`]）。
pub fn redraw(panel: PanelId, cx: &mut App) {
    let state = cx.default_global::<PanelControl>().panels.entry(panel);
    state.or_default().redraw = true;
}

/// 自分から変わったビュー（タイマー・非同期の結果）の `cx.notify()`。
/// フレームループはビューの notify を知らないので、[`redraw`] も頼む。
pub fn notify<V: 'static>(panel: PanelId, cx: &mut Context<V>) {
    cx.notify();
    redraw(panel, cx);
}

/// 全体の設定の変更を積む（フレームループが次の [`sync_settings`] で反映する）。
pub fn set(setting: Setting, cx: &mut App) {
    let control = cx.default_global::<PanelControl>();
    control.setting_changes.push(setting);
}

/// 今の全体の設定（フレームループが書き戻したもの。無ければ既定）。
pub fn settings(cx: &App) -> Settings {
    let control = cx.try_global::<PanelControl>();
    control.map(|c| c.settings).unwrap_or_default()
}

/// ビューが積んだ操作要求を `panel` に反映し、状態を書き戻す。変わったらウィンドウを描き直す（`true` を返す）。
/// 全体の設定（[`sync_settings`]）もここで反映するので、呼び出し側はパネルごとにこれだけを呼べばよい。
/// 入力を配送した後・描画する前に毎フレーム呼ぶ。
pub fn sync_panel<B: VrBackend>(kernel: &mut Kernel<B>, panel: PanelId, cx: &mut App) -> bool {
    sync_settings(kernel, cx);
    let ctl = cx.default_global::<PanelControl>();
    ctl.hands = kernel.hands().to_vec();
    ctl.hmd = kernel.hmd();
    ctl.controllers = kernel.controllers().to_vec();
    ctl.batteries = kernel.batteries().to_vec();
    ctl.media = kernel.now_playing();
    let settings = ctl.settings;
    let ctl = ctl.panels.entry(panel).or_default();
    ctl.pointing = kernel.pointing_hands(panel);
    for c in ctl.commands.drain(..) {
        kernel.apply(panel, c);
    }
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

/// ビューが積んだ全体の設定の変更をカーネルに反映し、今の設定を書き戻す。変わったら `true`。
/// テーマ・言語は GPUI のグローバルにも置く（画面は `design::Theme::current` / `i18n::t` で引く）。
pub fn sync_settings<B: VrBackend>(kernel: &mut Kernel<B>, cx: &mut App) -> bool {
    let ctl = cx.default_global::<PanelControl>();
    for s in ctl.setting_changes.drain(..) {
        kernel.set(s);
    }
    if ctl.os_keyboard_wanted {
        ctl.os_keyboard = kernel.os_keyboard();
    }
    let settings = kernel.settings();
    let changed = ctl.settings != settings;
    ctl.settings = settings;
    // 文字の大きさと太字化は GPUI の側（スレッドごと）に置く。そちらが食い違っていれば、設定が変わっていなくても合わせる。
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

/// カーネルの言語に対応する i18n の言語。
pub fn lang(l: Language) -> i18n::Lang {
    match l {
        Language::En => i18n::Lang::En,
        Language::Ja => i18n::Lang::Ja,
    }
}

/// i18n の言語に対応するカーネルの言語（[`lang`] の逆）。
pub fn language(l: i18n::Lang) -> Language {
    match l {
        i18n::Lang::En => Language::En,
        i18n::Lang::Ja => Language::Ja,
    }
}
