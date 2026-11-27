//! カーネル: 一点集中型の中枢。ブートローダー（`main`）が初期化し、UI や画面はこれだけを相手にする。
//!
//! 最下部の [`subsystems`] が OS・ランタイム・入力・描画の差を吸収し、その上で設定とパネル管理を行う。
//! フレームの進行（[`Kernel::tick`]）・パネルへの操作（[`Kernel::apply`]）・描画結果の受け取り
//! （[`Kernel::submit`]）を提供する。UI や画面は下位層を直接組み立てない。

mod audio_panels;
pub mod config;
mod desktop_panels;
mod os_keyboard;
pub use audio_panels::AudioStatus;
pub use desktop_panels::DesktopStats;
pub use subsystems::{self, Subsystems};
pub use subsystems::{audio, desktop, rendering, vr};

use std::sync::Arc;
use std::time::{Duration, Instant};
use subsystems::os;

pub use config::{
    COLOR_HISTORY, CURSOR_SCALE_MAX, CURSOR_SCALE_MIN, Config, DASHBOARD_DISTANCE,
    DASHBOARD_DISTANCE_MAX, DASHBOARD_DISTANCE_MIN, DASHBOARD_HEIGHT_MAX, DASHBOARD_HEIGHT_MIN,
    DASHBOARD_STEP, DominantHand, KEY_GAP_MAX, KEY_GAP_MIN, KEYBOARD_SCALE_MAX, KEYBOARD_SCALE_MIN,
    Language, LodQuality, NotificationAnchor, PersonalColor, PointerMode, PointerTuning,
    SCALE_STEP, TEXT_SCALE_MAX, TEXT_SCALE_MIN, ThemeMode, UI_SCALE_MAX, UI_SCALE_MIN,
    UpdateChannel, WRIST_SCALE_MAX, WRIST_SCALE_MIN, WristDesign, WristFace,
};
pub use overlay::{
    self, Anchor, DockHint, LazyFollow, Overlay, Panel, PanelId, Parent, Pointer, PointerEvent,
    ResizeLimits, UiEvent,
};
pub use subsystems::ControllerState;
pub use vr::{Device, Fov, Hand, Pose, Runtime, Vec2, VrBackend, VrError};

/// [`Kernel::tick`] 1 回の内訳。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TickTiming {
    /// ランタイムの poll（vsync 待ち・合成を含む）。
    pub poll: Duration,
    /// オーバーレイ層（追従・ヒット判定・ポインタの配置・アニメーション）。
    pub overlay: Duration,
    /// poll のうち、ランタイムがこのスレッドで合成・表示にかけた時間（novr のエミュレーターだけ）。
    pub compositor: Duration,
}

/// 上位層（ビュー）からパネルへの操作要求。
#[derive(Debug, Clone, PartialEq)]
pub enum PanelCommand {
    Attach(Device, Pose),
    /// その手のコントローラに、レーザーの射出点から前方へ伸ばして取り付ける（手を起こすと立って見える。大きければ縮小する。
    /// `Overlay::attach_to_hand`）。
    AttachToHand(Hand),
    Detach,
    Lock(bool),
    /// 頭の向き（左右だけ）と歩きにゆるく追従する（[`overlay::LazyFollow`]。`None` で外す）。
    LazyFollow(Option<LazyFollow>),
    /// このパネルがロックダウンを取得・解放する。取得中のパネルは自ら解放するまで閉じられず（[`PanelCommand::Close`] は無視される）、
    /// カーネルはロックダウン状態（[`Settings::lockdown`]）となる。シェル（ダッシュボードなど）はこの状態を参照して開閉を控える。
    /// 取得元は複数パネルでもよく、全パネルが解放（または閉鎖）するまでロックダウンは継続する。閉じる前に解放する場合は、同一パネルのコマンドに
    /// `Lockdown(false)`、`Close` の順で積む。設定ファイルには永続化しない。
    Lockdown(bool),
    /// 掴んで大きさを変えられるか（`Overlay::set_resizable`）。
    Resizable(bool),
    /// 角を掴んで縦横の解像度を変えられるようにする（`Overlay::set_corner_resize`。`None` で戻す）。
    /// 1m あたりのピクセル数を保つので、UI の表示領域が変わる。変えた解像度は範囲に収める。
    CornerResize(Option<ResizeLimits>),
    /// このパネルの上端を、画面をドッキングするスロットにする（上端からの傾斜角度。`None` で解除。`Overlay::set_dock_slot`）。
    DockSlot(Option<f32>),
    /// ドッキングスロットから掴んで取り外せる・近づけて離すとドッキングする画面にする（`Overlay::set_dockable`）。
    Dockable(bool),
    /// 取り付けたまま掴んで動かせる、取り付け先からの距離の上限（m。`Overlay::set_reach`。`None` で限らない）。
    Reach(Option<f32>),
    /// このパネルだけ、全体の設定によらずこの方式でポインタを流す（[`Kernel::panel_pointer_mode`]）。
    PointerMode(PointerMode),
    /// 掴んだとき、手で動かして頭へ向ける基準（このパネルから見た姿勢。`Overlay::set_grab_anchor`）。
    GrabAnchor(Option<Pose>),
    /// 掴む基準を付け替え、今の基準（無ければ中心）のあった所に新しい基準が来るようにパネルを動かす
    /// （`Overlay::move_grab_anchor`）。中身を組み直して基準の点（キーボードのホームポジション）が動いたときに使う。
    MoveGrabAnchor(Pose),
    /// このパネルのイン・アウトアニメーションを有効化・無効化する。
    Animated(bool),
    /// このパネルが出ている間、空間全体を暗くする（`Overlay::set_veil`）。
    Veil(bool),
    /// 子が開いているときと同じ暗さにする（`Overlay::set_dimmed`。入力は止めない）。案内（`ui::tour`）が、対象でないパネルを暗くする。
    Dim(bool),
    /// レーザーを透過させる（`Overlay::set_pass_through`。視界の端に配置する表示専用のパネル向け）。
    PassThrough(bool),
    /// 表示・非表示の遷移を不透明度のみで行う（`Overlay::set_fade_only`。アニメーションはパネル自身が描画する）。
    FadeOnly(bool),
    Close,
    /// 非表示・再表示を切り替える（`Overlay::set_hidden`。閉じずにビュー・配信・音声を維持する）。
    Hide(bool),
    /// その手のレーザーを描画するパネルにする（ヒット判定の対象外となり、配置はオーバーレイ層が決定する）。
    Pointer(Hand, Pointer),
    /// 解像度（UI の論理ピクセル）を変更する。1m あたりのピクセル数を維持するため VR 上の大きさも変化する。
    Resize(u32, u32),
    /// 現在の縦横比のまま、VR 上の高さだけを揃える。
    FitHeight(f32),
    /// 今の頭から見た相対姿勢（`ui::PanelSpec::in_view` と同じく頭の傾きは無視）へ置き直す。取り付けは外す。
    /// 姿勢は等倍のつもりの値で、掴む基準があれば基準がその所に来る（`Overlay::place`）。
    Place(Pose),
    /// 親子関係を設定する（`None` で独立）。子は親に追従し、子が開いている間の親は暗くなって入力を受けない。
    Parent(Option<Parent>),
    /// このパネルを、その持ち主のナビゲーションバーにする（持ち主の下端に付き、頭から見た大きさを保つ）。
    NavBar(PanelId),
    /// デスクトップの配信を開始・停止する。第2引数は取得の上限 FPS（1〜240）。
    Stream(Option<desktop::Source>, u32),
    /// 配信中のウィンドウを OS 上で前面化してフォーカスする（OS のキーボード・マウス操作が届くようにする）。
    /// ウィンドウ配信の開始時はカーネルが自動的に前面化するため、非表示にしていた配信を再表示する場合などに使用する。
    FocusSource,
    /// 配信中のウィンドウを OS 上で最小化する。
    MinimizeSource,
    /// このパネルのスピーカーで音声を再生する（`true` でループ再生）。読み込みは別スレッドで行い、複数同時に再生できる。
    Play(audio::Sound, bool),
    /// このパネルの中の点（パネルのピクセル、左上原点）から一度だけ音声を再生する。キーボードのキーのように、押した位置から聞こえる。
    /// `pitch` は速さ（1 = そのまま。上げると高く短くなる）、`volume` は音量（1 = そのまま、0 = 無音）。
    PlayAt {
        sound: audio::Sound,
        at: Vec2,
        pitch: f32,
        volume: f32,
    },
    /// このパネルを閉じるときにパネルの中央から一度再生する音声。閉じた後も再生終了までその位置から聞こえる。
    CloseSound(audio::Sound),
    /// このパネルで再生中の音声をすべて停止する。
    StopAudio,
    /// このパネルの音声を配置位置から聞こえるようにする（既定）。`false` でステレオのまま再生する。
    Spatial(bool),
    /// このパネルのスピーカーをモノラル・ステレオのどちらで再生するか（既定は音声に合わせる）。
    SpeakerLayout(audio::Layout),
    /// 仮想キーボードの一操作を OS の前面アプリへ送信する（このパネルが OS 操作用パネルの場合）。
    TypeToOs {
        stroke: subsystems::keyboard::Stroke,
        layout: subsystems::keyboard::Layout,
    },
}

/// 上位層（ビュー）からシステムへの要求。どのパネルにも属さない操作で、[`Kernel::request`] が処理する
/// （パネルへの操作は [`PanelCommand`]、全体設定の変更は [`Setting`]）。
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// 再生中のメディアを操作する（再生・一時停止・曲送り・シーク・アプリを前に出す。[`Kernel::now_playing`]）。
    Media(subsystems::media::Command),
    /// OS の通知 `id` の本文（`action` が `None`）か `action` 番目のボタンを押したことを、アプリへ届ける
    /// （[`Kernel::notifications`]。届けた通知は OS の通知センターからも消える）。URL を開くものは画面が開く。
    Notification { id: u32, action: Option<usize> },
    /// OS に通知を出す（デバッグのテスト用。`Notifications::send`）。
    SendNotification(subsystems::notifications::Outgoing),
    /// 通知をアクションなしで消去する（OS の通知センターから削除する。`Notifications::remove`）。
    RemoveNotification(u32),
}

/// アプリ全体の設定の変更（[`Kernel::set`]）。パネルを問わず効き、`PointerMode` と `AutoLaunch` 以外は設定ファイルへ書き戻される
/// （起動時の設定の場合のみ。テスト用のカーネルは書かない）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    // 設定ファイルに書かない値
    /// ランタイムの起動と同時に開くか（`VrBackend::set_auto_launch`）。値はランタイムが持つので設定ファイルには書かない。
    AutoLaunch(bool),
    // `[input]`
    /// コントローラを持たない手の調整値（`[input.hand]`）。範囲の外の値は範囲に収める。
    Hand(subsystems::input::HandTuning),
    // `[keyboard]`
    /// 仮想キーボードの種類（`keyboard.kind`）。
    Keyboard(subsystems::keyboard::KeyboardType),
    /// 仮想キーボードの大きさの倍率（`keyboard.scale`。範囲に収め、刻みに丸める）。カーネルは値を保持するのみで、解釈は UI 層が行う。
    KeyboardScale(f32),
    /// 仮想キーボードのキーの間隔の倍率（`keyboard.key_gap`。範囲に収め、刻みに丸める）。カーネルは値を保持するのみで、解釈は UI 層が行う。
    KeyGap(f32),
    /// 仮想キーボードのキー押下音を再生するか（`keyboard.key_sound`）。カーネルは値を保持するのみで、再生は UI 層が行う。
    KeySound(bool),
    /// キー押下音の音量（`keyboard.key_sound_volume`。0〜1 に収め、刻みに丸める）。カーネルは値を保持するのみ。
    KeySoundVolume(f32),
    // `[desktop]`
    /// ミラーリングの取得の上限 FPS（`desktop.frame_rate`。1〜240 に収める）。配信中のミラーもこの FPS でかけ直す。
    MirrorFrameRate(u32),
    // `[overlay]`
    /// UI スケール: パネルの VR 上の大きさの倍率（`overlay.ui_scale`。範囲に収め、刻みに丸める）。開いているパネルもその場で変わる。
    UiScale(f32),
    /// レーザーの手ブレ抑えの強さ（0 = 抑えない 〜 `overlay::POINTER_SMOOTHING_MAX`。`overlay.pointer_smoothing`）。
    PointerSmoothing(f32),
    /// 手ブレ抑えを緩める角度（度。`overlay.pointer_release_deg`）。
    PointerRelease(f32),
    /// レーザーの見える長さ（m。`overlay.laser_length`）。
    LaserLength(f32),
    /// カーソルの大きさの倍率（`overlay.cursor_scale`。範囲に収め、刻みに丸める）。
    CursorScale(f32),
    /// ダッシュボード表示中にゲームへの入力をブロックするか（`overlay.block_input`）。
    BlockInput(bool),
    /// ダッシュボード表示中の背景の暗さ（`overlay.veil_opacity`）。0 で暗くせず、1 で真っ黒になる。範囲に収め、刻みに丸める。
    /// 表示中に変更した場合も即座に反映される。
    DashboardDim(f32),
    // `[kernel]`
    /// 利き手（`kernel.dominant_hand`）。カーネルは値を保持するのみで、パネルの配置は変更しない（解釈は左右で動作が変わる側が行う）。
    DominantHand(DominantHand),
    /// レーザーのエミュレート方式（`kernel.pointer_mode`）。設定ファイルへは書き戻さない。
    PointerMode(PointerMode),
    /// 距離 LOD の品質（`kernel.lod_quality`）。
    LodQuality(LodQuality),
    // `[ui]`
    /// ツールチップを表示するか（`ui.tooltips`）。カーネルは値を保持するのみで、解釈は UI 層が行う。
    Tooltips(bool),
    /// タップ時（`ui.tap_sound`）・ホバー時（`ui.hover_sound`）の効果音を再生するか。カーネルは値を保持するのみで、解釈は UI 層が行う。
    TapSound(bool),
    HoverSound(bool),
    // `[design]`
    /// 見た目のテーマ（`design.theme`）。
    Theme(ThemeMode),
    /// パーソナルカラー（`design.personal_color`）。範囲の外の値は範囲に収める。
    PersonalColor(PersonalColor),
    /// 自作の色をパーソナルカラーに設定し、履歴（`design.color_history`）の先頭に記録する。同じ色は 1 つだけ残し、古いものから削除する。
    CustomColor(PersonalColor),
    /// 文字の大きさの倍率（`design.text_scale`。範囲に収め、刻みに丸める）。カーネルは値を保持するのみで、適用は UI 層が行う。
    TextScale(f32),
    /// すべての文字を太字にするか（`design.bold_text`）。カーネルは値を保持するのみで、適用は UI 層が行う。
    BoldText(bool),
    // `[i18n]`
    /// 画面の言語（`i18n.language`）。
    Language(Language),
    // `[wrist]`
    /// wrist の文字盤のデザイン（`wrist.design`）。
    WristDesign(WristDesign),
    /// wrist を掴んで再配置した姿勢（`wrist.offset`。左手首での値）。カーネルは値を保持するのみで、取り付けは wrist が行う。
    WristOffset(Pose),
    /// wrist の大きさの倍率（`wrist.scale`。範囲に収める）。カーネルは値を保持するのみで、解釈は wrist が行う。
    WristScale(f32),
    // `[toast]`
    /// 通知を視界の端に表示する位置（`toast.anchor`）。カーネルは値を保持するのみで、配置は通知画面が行う。
    NotificationAnchor(NotificationAnchor),
    // `[dashboard]`
    /// ダッシュボードの距離（m。`dashboard.distance`。範囲に収め、刻みに丸める）。カーネルは値を保持するのみで、配置はダッシュボードが行う。
    DashboardDistance(f32),
    /// ダッシュボードの高さ（m。目の高さの 0.05m 下からの相対で上が +。`dashboard.height`。範囲に収め、刻みに丸める）。
    DashboardHeight(f32),
    // `[settings]`
    /// 設定画面のポインタの動きの調整方式（`settings.pointer_tuning`）。カーネルは値を保持するのみで、解釈は設定画面が行う。
    PointerTuning(PointerTuning),
    // `[setup]`
    /// セットアップウィザードを完了したか（`setup.done`）。
    SetupDone(bool),
    // `[updater]`
    /// アップデートのチャンネル（`updater.channel`）。カーネルは値を保持するのみで、解釈は `updater` が行う。
    UpdateChannel(UpdateChannel),
}

/// アプリ全体の設定の今の値（[`Kernel::settings`]）。ビューの表示用。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    // 設定ファイルに書かない値
    /// ランタイムの起動と同時に開くか。切り替えられないランタイム（OpenVR 以外）は `None`。
    pub auto_launch: Option<bool>,
    // `[input]`
    pub hand: subsystems::input::HandTuning,
    // `[keyboard]`
    pub keyboard: subsystems::keyboard::KeyboardType,
    /// 仮想キーボードの大きさの倍率（等倍 = 1）。
    pub keyboard_scale: f32,
    /// 仮想キーボードのキーの間隔の倍率（等倍 = 1。0 でキーが接する）。
    pub key_gap: f32,
    /// 仮想キーボードのキーを押した音を鳴らす。
    pub key_sound: bool,
    /// キーを押した音の音量（0〜1）。
    pub key_sound_volume: f32,
    // `[desktop]`
    pub mirror_frame_rate: u32,
    // `[overlay]`
    /// UI スケール（パネルの大きさの倍率。等倍 = 1）。
    pub ui_scale: f32,
    pub pointer_smoothing: f32,
    pub pointer_release_deg: f32,
    pub laser_length: f32,
    /// カーソルの大きさの倍率（等倍 = 1）。
    pub cursor_scale: f32,
    pub block_input: bool,
    /// ダッシュボード表示中の背景の暗さ（黒の不透明度、0〜1）。
    pub dashboard_dim: f32,
    // `[kernel]`
    pub dominant_hand: DominantHand,
    pub pointer_mode: PointerMode,
    pub lod_quality: LodQuality,
    // `[ui]`
    pub tooltips: bool,
    pub tap_sound: bool,
    pub hover_sound: bool,
    // `[design]`
    pub theme: ThemeMode,
    /// パーソナルカラー。選んでいなければ `None`（既定の色味）。
    pub personal_color: Option<PersonalColor>,
    /// 自分で作ったパーソナルカラーの履歴（新しい順。空きは `None`）。
    pub color_history: [Option<PersonalColor>; COLOR_HISTORY],
    /// 文字の大きさの倍率（等倍 = 1）。
    pub text_scale: f32,
    /// すべての文字を太字にする。
    pub bold_text: bool,
    // `[i18n]`
    pub language: Language,
    // `[wrist]`
    pub wrist_design: WristDesign,
    /// wrist を掴んで置き直した姿勢（左手首での値）。置き直していない・設定ファイルの値が壊れていれば `None`。
    pub wrist_offset: Option<Pose>,
    /// wrist の大きさの倍率（等倍 = 1）。
    pub wrist_scale: f32,
    // `[toast]`
    pub notification_anchor: NotificationAnchor,
    // `[dashboard]`
    /// ダッシュボードの距離（m。はめた画面の中心まで）。
    pub dashboard_distance: f32,
    /// ダッシュボードの高さ（m。上が +。0 で画面の中心が目の高さの 0.05m 下）。
    pub dashboard_height: f32,
    // `[settings]`
    pub pointer_tuning: PointerTuning,
    // `[setup]`
    pub setup_done: bool,
    // `[updater]`
    pub update_channel: UpdateChannel,
    // 設定ではなく、カーネルの今の状態
    /// ロックダウン中か（[`Kernel::lockdown`]）。シェル（ダッシュボードなど）はこれを見て、開閉を控える。
    pub lockdown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_launch: None,
            hand: Default::default(),
            keyboard: Default::default(),
            keyboard_scale: 1.0,
            key_gap: 1.0,
            key_sound: true,
            key_sound_volume: 1.0,
            mirror_frame_rate: desktop::DEFAULT_FRAME_RATE,
            ui_scale: 1.0,
            pointer_smoothing: Default::default(),
            pointer_release_deg: Default::default(),
            laser_length: Default::default(),
            cursor_scale: 1.0,
            block_input: true,
            dashboard_dim: overlay::VEIL,
            dominant_hand: Default::default(),
            pointer_mode: Default::default(),
            lod_quality: Default::default(),
            tooltips: true,
            tap_sound: true,
            hover_sound: true,
            theme: Default::default(),
            personal_color: None,
            color_history: [None; COLOR_HISTORY],
            text_scale: 1.0,
            bold_text: false,
            language: Default::default(),
            wrist_design: Default::default(),
            wrist_offset: None,
            wrist_scale: 1.0,
            notification_anchor: Default::default(),
            dashboard_distance: DASHBOARD_DISTANCE,
            dashboard_height: 0.0,
            pointer_tuning: Default::default(),
            setup_done: false,
            update_channel: Default::default(),
            lockdown: false,
        }
    }
}

/// パネルの状態。ビューの表示用。全体の設定は [`Settings`]。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PanelStatus {
    pub runtime: Option<Runtime>,
    pub attached: Option<Device>,
    pub locked: bool,
    pub animated: bool,
    /// 見えているトラッカーの ID（取り付け先の候補）。
    pub trackers: Vec<String>,
    /// メニューボタンを押している。
    pub menu: bool,
    /// ショートカットボタン（左手の X）を二回続けて押した回数（全パネル共通。起動から数える）。
    /// 押した瞬間を取りこぼさないようカーネルが毎 tick 数え、何をするかは画面が決める（wrist はダッシュボードを開く・閉じる）。
    pub double_press: u32,
    /// どのパネルにも当たっていない所でトリガーを押した回数（全パネル共通。起動から数える）。何をするかは画面が決める
    /// （ダッシュボードは出ていれば隠す）。
    pub outside_press: u32,
    /// パネルを掴んでいる。
    pub grabbed: bool,
    /// 閉じられた。
    pub closed: bool,
    /// 隠している（[`PanelCommand::Hide`]）。
    pub hidden: bool,
    /// 描画解像度の段階（[`Kernel::panel_lod`]）。
    pub lod: u8,
    /// 親パネル（このパネルが子のとき）。
    pub parent: Option<PanelId>,
    /// レーザーが指している（`Overlay::pointed`。ナビゲーションバーは持ち主を指していても）。
    pub pointed: bool,
    /// 見ている（`Overlay::gazed`。頭の正面に近く、面がこちらを向いている。腕時計を見る構え）。
    pub gazed: bool,
    pub desktop: Option<desktop::Status>,
    /// 配信中なら、直近一秒の統計（最初の一秒は `None`）。
    pub desktop_stats: Option<DesktopStats>,
    /// このパネルのスピーカー。
    pub audio: AudioStatus,
    /// ドッキングスロットの見本（[`Pointer::Dock`]）の場合、その手で掴んでいる画面に表示する一行（`Overlay::dock_hint`）。
    pub dock_hint: Option<DockHint>,
    /// 角の線（[`Pointer::Corner`]）の場合、その手で角を掴んで大きさを変えている途中か（`Overlay::corner_held`）。
    pub corner_held: bool,
}

/// 中枢。本番は `Kernel`（= `Kernel<Box<dyn VrBackend>>`）、テストは具象のバックエンド（モック等）を持てる。
pub struct Kernel<B: VrBackend = Box<dyn VrBackend>> {
    pub config: Config,
    /// テストがモックを直接操作するために公開している。本番コードはメソッド経由で使用する。
    pub subsystems: Subsystems<B>,
    overlay: Overlay,
    timing: TickTiming,
    pointer_mode: PointerMode,
    /// テーマの変更を書き戻す設定ファイル。`boot` のみが設定する（テスト用のカーネルは実ファイルに触れない）。
    config_path: Option<std::path::PathBuf>,
    /// 進めたフレームの数（[`Self::panel_frame_due`]）。
    frame: u64,
    /// パネルごとの LOD（[`Self::panel_lod`]）。tick ごとに前の値から決める。
    lods: Vec<u8>,
    desktop_panels: std::collections::HashMap<PanelId, desktop_panels::DesktopPanel>,
    /// 音声出力。テスト用のカーネルはデバイスを開かず（`audio::Engine::offline`）、テストが `render` で駆動する。
    pub audio: audio::Engine,
    /// 音声を再生・設定したパネルのスピーカー。
    speakers: std::collections::HashMap<PanelId, audio_panels::SpeakerConfig>,
    /// パネルごとの、次に使う点の音のスピーカー（[`PanelCommand::PlayAt`]）。
    point_slots: std::collections::HashMap<PanelId, u64>,
    /// 閉じるときに再生する音声（[`PanelCommand::CloseSound`]）。
    close_sounds: std::collections::HashMap<PanelId, audio::Sound>,
    /// 全体の設定によらないポインタの方式（[`PanelCommand::PointerMode`]）。
    pointer_modes: std::collections::HashMap<PanelId, PointerMode>,
    /// ロックダウンを取っているパネル（[`PanelCommand::Lockdown`]）。
    lockdown_holders: std::collections::BTreeSet<PanelId>,
    /// ショートカットボタンの二回連続押し（[`PanelStatus::double_press`]）。
    shortcut: DoublePress,
    /// ランタイムが数えた二回押しの合計（[`vr::DOUBLE_PRESS_INPUT`]）。
    runtime_double_press: u32,
    /// 骨格だけの手のショートカット（親指と小指）の長押し。二回押しと同じく [`PanelStatus::double_press`] に数える。
    hand_shortcut: LongPress,
    /// OS の前面アプリのキーボードの監視と入力（[`Kernel::os_keyboard`]。使用開始まで初期化しない）。
    os_keyboard: Option<os::keyboard::Watcher>,
    /// 配信中のウィンドウが前面化するのを待つ OS への入力（[`os_keyboard::PendingKey`]）。
    pending_os_input: Vec<os_keyboard::PendingKey>,
    /// 最後の tick の電池（[`Kernel::batteries`]）。
    batteries: Vec<vr::Battery>,
    /// 再生中のメディアの監視（[`Kernel::now_playing`]）。`boot` のみが初期化する（テスト用のカーネルは OS に触れない）。
    media: Option<subsystems::media::Media>,
    /// OS の通知の監視（[`Kernel::notifications`]）。`boot` のみが初期化する。
    notifications: Option<subsystems::notifications::Notifications>,
}

/// 二回続けて押したと見なす、押下と次の押下の間隔の上限。
pub const DOUBLE_PRESS: Duration = Duration::from_millis(250);
/// 骨格だけの手のショートカット（親指と小指を付ける）を、この時間押し続けると一回と数える。指を付け直す二回押しは、
/// ハンドトラッキングでは離したかどうかが曖昧で数え損ねるので、手は長押しにする。
pub const HAND_LONG_PRESS: Duration = Duration::from_millis(500);

/// 押し続けて [`HAND_LONG_PRESS`] たったら一回数える（離すまで次は数えない）。
#[derive(Debug, Default)]
struct LongPress {
    since: Option<Instant>,
    fired: bool,
    count: u32,
}

impl LongPress {
    fn update(&mut self, held: bool, now: Instant) {
        if !held {
            (self.since, self.fired) = (None, false);
            return;
        }
        let since = *self.since.get_or_insert(now);
        if !self.fired && now.duration_since(since) >= HAND_LONG_PRESS {
            self.fired = true;
            self.count += 1;
            logger::debug!("hand shortcut held", count = self.count);
        }
    }
}

/// 押下の立ち上がりを見て、[`DOUBLE_PRESS`] 以内に二回続いたら数える。三回目は次の一回目として数え直す。
#[derive(Debug, Default)]
struct DoublePress {
    held: bool,
    first: Option<Instant>,
    count: u32,
}

impl DoublePress {
    fn update(&mut self, held: bool, now: Instant) {
        let pressed = held && !self.held;
        self.held = held;
        if !pressed {
            return;
        }
        let since = self.first.take().map(|t| now.duration_since(t));
        match since {
            Some(d) if d <= DOUBLE_PRESS => self.count += 1,
            _ => self.first = Some(now),
        }
        logger::debug!("shortcut button pressed", since_first = ?since, count = self.count);
    }
}

/// LOD の切り替え距離を決めた基準の密度（1m あたりの論理ピクセル。既定のパネル 800px / 0.8m）。
/// 拡大して密度が下がったパネルは、そのぶん近くにあるものとして LOD を決める（見た目の粗さは距離 / 密度で決まる）。
const LOD_PX_PER_M: f32 = 1000.0;
/// LOD を下げる（近づいて解像度を上げる）のは、閾値のこの割合まで近づいてから。
const LOD_HYSTERESIS: f32 = 0.9;

impl Kernel {
    /// ランタイムを選んで起動する。`requested` は `"--novr"` / `"novr"` のような指定（コマンドライン引数から呼び出し側が渡す）。
    /// 指定なしで VR ランタイムが無ければ、`novr_fallback` なら novr で起動する（`novr` フィーチャーがあれば常に使える）、でなければエラー。
    /// 指定したのに繋がらなければエラー。
    pub fn boot(
        requested: Option<&str>,
        mut config: Config,
        novr_fallback: bool,
    ) -> Result<Self, VrError> {
        // 言語を決めていなければ、OS の表示言語で始める（決めるまで設定ファイルには書かない）。
        if config.i18n.language.is_none() {
            config.i18n.language = os::ui_language().and_then(Language::from_code);
        }
        let prefer = match requested.map(runtime_name).as_deref() {
            None | Some("") => None,
            Some("novr") => Some(Runtime::NoVr),
            Some("openxr") => Some(Runtime::OpenXr),
            Some("openvr") => Some(Runtime::OpenVr),
            // 読めない指定を「指定なし」として扱うと、VR ランタイムに繋ぎに行って SteamVR を起こしてしまう。
            Some(_) => {
                logger::error!("unknown runtime", requested = requested.unwrap_or_default());
                return Err(VrError::NoRuntime);
            }
        };
        let vr = match vr::connect(prefer, config.vr.emulator_gpu) {
            Ok(b) => b,
            Err(e) if prefer.is_none() && novr_fallback => {
                logger::warn!("failed to connect to VR runtime, starting novr", error = e);
                // `novr` フィーチャーなしのビルドにはエミュレーターが無く、ここで失敗しうる。
                vr::connect(Some(Runtime::NoVr), config.vr.emulator_gpu)?
            }
            Err(e) => {
                logger::error!("failed to connect to VR runtime", runtime = ?prefer, error = e);
                return Err(e);
            }
        };
        logger::info!("runtime selected", runtime = ?vr.runtime());
        let mut kernel = Self::with_backend(vr, config);
        kernel.config_path = config::path();
        kernel.audio = audio::Engine::open();
        kernel.media = Some(subsystems::media::Media::start());
        kernel.set_notifications(subsystems::notifications::Notifications::start());
        Ok(kernel)
    }
}

/// ランタイムの指定を比べられる形にする（`--NoVr` / `no-vr` / `no_vr` → `novr`）。
fn runtime_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl<B: VrBackend> Kernel<B> {
    /// 接続済みのバックエンドで起動する（テスト用。本番は [`Kernel::boot`]）。
    pub fn with_backend(vr: B, config: Config) -> Self {
        // フレームの周期を作る短い待ちが丸められないように。
        os::precise_timers();
        // ゲームが GPU を使い切っていても、パネルの描画と読み戻しが後回しにされないように。
        if !os::high_gpu_priority() {
            logger::warn!("cannot raise GPU scheduling priority");
        }
        let mut subsystems = Subsystems::with_backend(vr);
        subsystems.hand = config.input.hand.clamped();
        let mut overlay = Overlay::new();
        if let Some(s) = config.overlay.pointer_smoothing {
            overlay.set_pointer_smoothing(s);
        }
        if let Some(deg) = config.overlay.pointer_release_deg {
            overlay.set_pointer_release(deg);
        }
        if let Some(l) = config.overlay.laser_length {
            overlay.set_laser_length(l);
        }
        if let Some(s) = config.overlay.cursor_scale {
            overlay.set_cursor_scale(config::clamp_scale(s, CURSOR_SCALE_MIN, CURSOR_SCALE_MAX));
        }
        overlay.set_block_input(config.overlay.block_input.unwrap_or(true));
        if let Some(d) = config.overlay.veil_opacity {
            overlay.set_veil_opacity(config::snap(d, 0.0, 1.0, SCALE_STEP, overlay::VEIL));
        }
        if let Some(s) = config.overlay.ui_scale {
            let s = config::clamp_scale(s, UI_SCALE_MIN, UI_SCALE_MAX);
            overlay.set_scale(&mut subsystems.backend, s);
        }
        let pointer_mode = config.kernel.pointer_mode.unwrap_or_default();
        // タッチではスライドがパネルからはみ出しても続くよう、押している間はポインタを掴んでおく。
        overlay.set_capture(pointer_mode == PointerMode::Touch);
        Self {
            pointer_mode,
            config_path: None,
            config,
            subsystems,
            overlay,
            timing: TickTiming::default(),
            frame: 0,
            lods: Vec::new(),
            desktop_panels: Default::default(),
            audio: audio::Engine::offline(48_000, false),
            speakers: Default::default(),
            point_slots: Default::default(),
            close_sounds: Default::default(),
            pointer_modes: Default::default(),
            lockdown_holders: Default::default(),
            shortcut: DoublePress::default(),
            runtime_double_press: 0,
            hand_shortcut: LongPress::default(),
            os_keyboard: None,
            pending_os_input: Vec::new(),
            batteries: Vec::new(),
            media: None,
            notifications: None,
        }
    }

    pub fn runtime(&self) -> Runtime {
        self.subsystems.backend.runtime()
    }

    /// レーザーのエミュレート方式。UI がポインタイベントをマウス・タッチのどちらとして流すかを決める。
    pub fn pointer_mode(&self) -> PointerMode {
        self.pointer_mode
    }

    /// `panel` でのレーザーのエミュレート方式（[`PanelCommand::PointerMode`] で決めていなければ全体の設定）。
    pub fn panel_pointer_mode(&self, panel: PanelId) -> PointerMode {
        self.pointer_modes
            .get(&panel)
            .copied()
            .unwrap_or(self.pointer_mode)
    }

    /// ロックダウン中か: 開いているパネルのどれかがロックダウンを取っている（[`PanelCommand::Lockdown`]）。
    /// 取ったパネルが閉じれば自然に外れる。
    pub fn lockdown(&self) -> bool {
        self.lockdown_holders
            .iter()
            .any(|&p| !self.overlay.panel(p).closed)
    }

    pub fn settings(&self) -> Settings {
        Settings {
            auto_launch: self.subsystems.backend.auto_launch(),
            hand: self.subsystems.hand,
            keyboard: self.config.keyboard.kind,
            keyboard_scale: self.config.keyboard.scale.map_or(1.0, |s| {
                config::clamp_scale(s, KEYBOARD_SCALE_MIN, KEYBOARD_SCALE_MAX)
            }),
            key_gap: self
                .config
                .keyboard
                .key_gap
                .map_or(1.0, |g| config::clamp_scale(g, KEY_GAP_MIN, KEY_GAP_MAX)),
            key_sound: self.config.keyboard.key_sound.unwrap_or(true),
            key_sound_volume: self
                .config
                .keyboard
                .key_sound_volume
                .map_or(1.0, |v| config::clamp_scale(v, 0.0, 1.0)),
            mirror_frame_rate: self.mirror_frame_rate(),
            ui_scale: self.overlay.scale(),
            pointer_smoothing: self.overlay.pointer_smoothing(),
            pointer_release_deg: self.overlay.pointer_release(),
            laser_length: self.overlay.laser_length(),
            cursor_scale: self.overlay.cursor_scale(),
            block_input: self.overlay.block_input(),
            dashboard_dim: self.overlay.veil_opacity(),
            dominant_hand: self.config.kernel.dominant_hand,
            pointer_mode: self.pointer_mode,
            lod_quality: self.config.kernel.lod_quality,
            tooltips: self.config.ui.tooltips.unwrap_or(true),
            tap_sound: self.config.ui.tap_sound.unwrap_or(true),
            hover_sound: self.config.ui.hover_sound.unwrap_or(true),
            theme: self.config.design.theme,
            personal_color: self
                .config
                .design
                .personal_color
                .map(PersonalColor::clamped),
            color_history: std::array::from_fn(|i| {
                let color = self.config.design.color_history.get(i);
                color.copied().map(PersonalColor::clamped)
            }),
            text_scale: self.config.design.text_scale.map_or(1.0, |s| {
                config::clamp_scale(s, TEXT_SCALE_MIN, TEXT_SCALE_MAX)
            }),
            bold_text: self.config.design.bold_text,
            language: self.config.i18n.language.unwrap_or_default(),
            wrist_design: self.config.wrist.design,
            wrist_offset: self.config.wrist.offset.and_then(config::WristOffset::pose),
            wrist_scale: self.wrist_scale(),
            notification_anchor: self.config.toast.anchor,
            dashboard_distance: self.dashboard_distance(),
            dashboard_height: self.dashboard_height(),
            pointer_tuning: self.config.settings.pointer_tuning,
            setup_done: self.config.setup.done,
            update_channel: self.config.updater.channel,
            lockdown: self.lockdown(),
        }
    }

    /// ダッシュボードの距離（設定ファイルの値を範囲に収め、刻みに丸めたもの）。
    fn dashboard_distance(&self) -> f32 {
        let (min, max) = (DASHBOARD_DISTANCE_MIN, DASHBOARD_DISTANCE_MAX);
        let d = self.config.dashboard.distance.unwrap_or(DASHBOARD_DISTANCE);
        config::snap(d, min, max, DASHBOARD_STEP, DASHBOARD_DISTANCE)
    }

    /// ダッシュボードの高さ（設定ファイルの値を範囲に収め、刻みに丸めたもの）。
    fn dashboard_height(&self) -> f32 {
        let (min, max) = (DASHBOARD_HEIGHT_MIN, DASHBOARD_HEIGHT_MAX);
        let h = self.config.dashboard.height.unwrap_or(0.0);
        config::snap(h, min, max, DASHBOARD_STEP, 0.0)
    }

    /// wrist の大きさの倍率（設定ファイルの値を範囲に収めたもの。数でない値は等倍）。
    fn wrist_scale(&self) -> f32 {
        let s = self.config.wrist.scale.filter(|s| s.is_finite());
        s.map_or(1.0, |s| s.clamp(WRIST_SCALE_MIN, WRIST_SCALE_MAX))
    }

    /// UI スケール（[`Setting::UiScale`]）。パネルの大きさ（[`Self::add_panel`] など）は等倍のつもりで渡し、これが掛かる。
    pub fn ui_scale(&self) -> f32 {
        self.overlay.scale()
    }

    /// ミラーリングの取得の上限 FPS（設定ファイルの値を 1〜240 に収めたもの）。
    fn mirror_frame_rate(&self) -> u32 {
        self.config
            .desktop
            .frame_rate
            .unwrap_or(desktop::DEFAULT_FRAME_RATE)
            .clamp(1, desktop::MAX_FRAME_RATE)
    }

    /// アプリ全体の設定を変更する。
    pub fn set(&mut self, setting: Setting) {
        let (table, key, value) = match setting {
            Setting::AutoLaunch(on) => {
                self.subsystems.backend.set_auto_launch(on);
                return;
            }
            Setting::Hand(h) => {
                let h = h.clamped();
                self.subsystems.hand = h;
                self.config.input.hand = h;
                ("input", "hand", config::value(h))
            }
            Setting::Keyboard(k) => {
                self.config.keyboard.kind = k;
                ("keyboard", "kind", config::value(k))
            }
            Setting::KeyboardScale(s) => {
                let s = config::clamp_scale(s, KEYBOARD_SCALE_MIN, KEYBOARD_SCALE_MAX);
                self.config.keyboard.scale = Some(s);
                ("keyboard", "scale", config::value(s))
            }
            Setting::KeyGap(g) => {
                let g = config::clamp_scale(g, KEY_GAP_MIN, KEY_GAP_MAX);
                self.config.keyboard.key_gap = Some(g);
                ("keyboard", "key_gap", config::value(g))
            }
            Setting::KeySound(b) => {
                self.config.keyboard.key_sound = Some(b);
                ("keyboard", "key_sound", config::value(b))
            }
            Setting::KeySoundVolume(v) => {
                let v = config::clamp_scale(v, 0.0, 1.0);
                self.config.keyboard.key_sound_volume = Some(v);
                ("keyboard", "key_sound_volume", config::value(v))
            }
            Setting::MirrorFrameRate(f) => {
                self.config.desktop.frame_rate = Some(f);
                let f = self.mirror_frame_rate();
                self.config.desktop.frame_rate = Some(f);
                self.restart_desktops(f);
                ("desktop", "frame_rate", config::value(f))
            }
            Setting::UiScale(s) => {
                let s = config::clamp_scale(s, UI_SCALE_MIN, UI_SCALE_MAX);
                self.overlay.set_scale(&mut self.subsystems.backend, s);
                self.config.overlay.ui_scale = Some(s);
                ("overlay", "ui_scale", config::value(s))
            }
            Setting::PointerSmoothing(s) => {
                self.overlay.set_pointer_smoothing(s);
                let s = self.overlay.pointer_smoothing();
                self.config.overlay.pointer_smoothing = Some(s);
                ("overlay", "pointer_smoothing", config::value(s))
            }
            Setting::PointerRelease(deg) => {
                self.overlay.set_pointer_release(deg);
                let deg = self.overlay.pointer_release();
                self.config.overlay.pointer_release_deg = Some(deg);
                ("overlay", "pointer_release_deg", config::value(deg))
            }
            Setting::LaserLength(l) => {
                self.overlay.set_laser_length(l);
                let l = self.overlay.laser_length();
                self.config.overlay.laser_length = Some(l);
                ("overlay", "laser_length", config::value(l))
            }
            Setting::CursorScale(s) => {
                let s = config::clamp_scale(s, CURSOR_SCALE_MIN, CURSOR_SCALE_MAX);
                self.overlay.set_cursor_scale(s);
                self.config.overlay.cursor_scale = Some(s);
                ("overlay", "cursor_scale", config::value(s))
            }
            Setting::BlockInput(b) => {
                self.overlay.set_block_input(b);
                self.config.overlay.block_input = Some(b);
                ("overlay", "block_input", config::value(b))
            }
            Setting::DashboardDim(d) => {
                let d = config::snap(d, 0.0, 1.0, SCALE_STEP, overlay::VEIL);
                self.overlay.set_veil_opacity(d);
                self.config.overlay.veil_opacity = Some(d);
                ("overlay", "veil_opacity", config::value(d))
            }
            Setting::DominantHand(h) => {
                self.config.kernel.dominant_hand = h;
                ("kernel", "dominant_hand", config::value(h))
            }
            Setting::PointerMode(m) => {
                self.pointer_mode = m;
                self.overlay.set_capture(m == PointerMode::Touch);
                return;
            }
            Setting::LodQuality(q) => {
                self.config.kernel.lod_quality = q;
                ("kernel", "lod_quality", config::value(q))
            }
            Setting::Tooltips(b) => {
                self.config.ui.tooltips = Some(b);
                ("ui", "tooltips", config::value(b))
            }
            Setting::TapSound(b) => {
                self.config.ui.tap_sound = Some(b);
                ("ui", "tap_sound", config::value(b))
            }
            Setting::HoverSound(b) => {
                self.config.ui.hover_sound = Some(b);
                ("ui", "hover_sound", config::value(b))
            }
            Setting::Theme(t) => {
                self.config.design.theme = t;
                ("design", "theme", config::value(t))
            }
            Setting::PersonalColor(c) => {
                let c = c.clamped();
                self.config.design.personal_color = Some(c);
                ("design", "personal_color", config::value(c))
            }
            Setting::CustomColor(c) => {
                // 選ぶのは `PersonalColor` と同じ（先に書き戻す）。続けて履歴を書き戻す。
                self.set(Setting::PersonalColor(c));
                let c = c.clamped();
                let history = &mut self.config.design.color_history;
                history.retain(|h| h.clamped() != c);
                history.insert(0, c);
                history.truncate(COLOR_HISTORY);
                ("design", "color_history", config::value(history.clone()))
            }
            Setting::TextScale(s) => {
                let s = config::clamp_scale(s, TEXT_SCALE_MIN, TEXT_SCALE_MAX);
                self.config.design.text_scale = Some(s);
                ("design", "text_scale", config::value(s))
            }
            Setting::BoldText(b) => {
                self.config.design.bold_text = b;
                ("design", "bold_text", config::value(b))
            }
            Setting::Language(l) => {
                self.config.i18n.language = Some(l);
                ("i18n", "language", config::value(l))
            }
            Setting::WristDesign(d) => {
                self.config.wrist.design = d;
                ("wrist", "design", config::value(d))
            }
            Setting::WristOffset(p) => {
                let o = config::WristOffset::from(p);
                self.config.wrist.offset = Some(o);
                ("wrist", "offset", config::value(o))
            }
            Setting::WristScale(s) => {
                self.config.wrist.scale = Some(s);
                let s = self.wrist_scale();
                self.config.wrist.scale = Some(s);
                ("wrist", "scale", config::value(s))
            }
            Setting::NotificationAnchor(a) => {
                self.config.toast.anchor = a;
                ("toast", "anchor", config::value(a))
            }
            Setting::DashboardDistance(d) => {
                self.config.dashboard.distance = Some(d);
                let d = self.dashboard_distance();
                self.config.dashboard.distance = Some(d);
                ("dashboard", "distance", config::value(d))
            }
            Setting::DashboardHeight(h) => {
                self.config.dashboard.height = Some(h);
                let h = self.dashboard_height();
                self.config.dashboard.height = Some(h);
                ("dashboard", "height", config::value(h))
            }
            Setting::PointerTuning(t) => {
                self.config.settings.pointer_tuning = t;
                ("settings", "pointer_tuning", config::value(t))
            }
            Setting::SetupDone(b) => {
                self.config.setup.done = b;
                ("setup", "done", config::value(b))
            }
            Setting::UpdateChannel(c) => {
                self.config.updater.channel = c;
                ("updater", "channel", config::value(c))
            }
        };
        if let Some(path) = &self.config_path
            && let Err(e) = config::save_at(path, table, key, value)
        {
            logger::warn!(
                "failed to save to config file",
                table = table,
                key = key,
                path = path.display(),
                error = e
            );
        }
    }

    pub fn add_panel(
        &mut self,
        key: &str,
        pose: Pose,
        size: Vec2,
        resolution: (u32, u32),
    ) -> Result<PanelId, VrError> {
        self.overlay
            .add_panel(&mut self.subsystems.backend, key, pose, size, resolution)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        self.overlay.panel(id)
    }

    /// `owner` に付いている開いたナビゲーションバー。
    pub fn nav_bar(&self, owner: PanelId) -> Option<PanelId> {
        self.overlay.present_ids().iter().copied().find(|&i| {
            let p = self.overlay.panel(i);
            !p.closed && p.nav_bar_of == Some(owner)
        })
    }

    /// 距離による描画解像度の段階（LOD0〜3）。Mid は 2 / 4 / 8 m で切り替える（下げるのは [`LOD_HYSTERESIS`] だけ近づいてから）。
    /// 距離は密度で換算する（[`LOD_PX_PER_M`]）。2 倍に拡大したパネルは半分の距離として扱う。
    /// [`Self::tick`] で決めた値（まだ tick していないパネルはその場で求める）。
    pub fn panel_lod(&self, id: PanelId) -> u8 {
        self.lods
            .get(id)
            .copied()
            .unwrap_or_else(|| self.lod_for(id, None))
    }

    /// 前の段階 `prev` から、距離で次の段階を決める。境目の前後で行き来しないよう、下げる側は閾値を縮める。
    /// レーザーが指している（操作しようとしている）パネルとツールチップ、はまる所の見本、角の線は、距離によらず LOD0。
    fn lod_for(&self, id: PanelId, prev: Option<u8>) -> u8 {
        if self.overlay.pointed(id)
            || self.panel(id).pointer.is_some_and(|(_, part)| {
                matches!(part, Pointer::Tooltip | Pointer::Dock | Pointer::Corner)
            })
        {
            return 0;
        }
        let near = match self.config.kernel.lod_quality {
            LodQuality::Off => return 0,
            LodQuality::Low => 1.0,
            LodQuality::Mid => 2.0,
            LodQuality::High => 4.0,
        };
        let p = self.panel(id);
        let density = p.resolution.0 as f32 / p.size.x / LOD_PX_PER_M;
        let distance = p.pose.position.distance(self.hmd().position) * density;
        let count = |scale: f32| {
            [near, near * 2.0, near * 4.0]
                .into_iter()
                .filter(|&threshold| distance >= threshold * scale)
                .count() as u8
        };
        let (up, down) = (count(1.0), count(LOD_HYSTERESIS));
        match prev {
            // 上げるのは閾値ちょうどで、下げるのは縮めた閾値を割ってから。
            Some(prev) => prev.clamp(up, down),
            None => up,
        }
    }

    /// このフレームでパネルを描き直してよいか。LOD0 は毎フレーム、LOD n は 2^n フレームに 1 回。
    /// パネルごとにずらし、間引いたパネルの描き直しが同じフレームに重ならないようにする。
    pub fn panel_frame_due(&self, id: PanelId) -> bool {
        (self.frame + id as u64).is_multiple_of(1 << self.panel_lod(id))
    }

    /// 1 フレーム進める。ランタイムを poll し、パネルの追従・掴み移動とレーザーのヒット判定を行って UI イベントを返す。
    pub fn tick(&mut self) -> Vec<UiEvent> {
        let t = Instant::now();
        self.frame += 1;
        let frame = self.subsystems.poll();
        let polled = Instant::now();
        self.poll_desktop();
        self.flush_os_input();
        // ボタン（左手の X）は二回押し、骨格だけの手（向きが分かる）は長押し。
        let held = |hand: bool| {
            frame
                .controllers
                .iter()
                .any(|c| c.shortcut && c.facing.is_some() == hand)
        };
        self.shortcut.update(held(false), polled);
        self.runtime_double_press +=
            frame.controllers.iter().filter(|c| c.double_press).count() as u32;
        self.hand_shortcut.update(held(true), polled);
        self.batteries.clone_from(&frame.batteries);
        let mut events = self.overlay.tick_frame(&mut self.subsystems.backend, frame);
        self.route_desktop(&mut events);
        self.update_audio();
        // 段階が変わるとパネルの描き直し・描画先とランタイムのテクスチャの作り直しが走るので、境目で行き来させない。
        let old_len = self.lods.len();
        self.lods.resize(self.overlay.len(), 0);
        for &id in self.overlay.present_ids() {
            if !self.overlay.panel(id).closed {
                let prev = (id < old_len).then_some(self.lods[id]);
                self.lods[id] = self.lod_for(id, prev);
            }
        }
        self.timing = TickTiming {
            poll: polled - t,
            overlay: polled.elapsed(),
            compositor: self.subsystems.backend.compositor_time(),
        };
        events
    }

    /// 最後の [`tick`](Self::tick) の内訳（プロファイラ用）。
    pub fn timing(&self) -> TickTiming {
        self.timing
    }

    /// フレームの周期が決まっていれば（novr）その長さ。プロファイラがドロップを数える基準に使う。
    pub fn frame_interval(&self) -> Option<Duration> {
        self.subsystems.backend.frame_interval()
    }

    /// `hand` が押している間のポインタのキャプチャを外す（タッチの長押しでドラッグになったとき。ほかのパネルへ運べるように）。
    pub fn release_capture(&mut self, hand: Hand) {
        self.overlay.release_capture(hand);
    }

    /// パネルが見えているか（ほかのパネルに隠れていない・視野の中）。ランタイムが直前の合成から教えてくれればそれ（novr）、
    /// 無ければ頭の向きと視野で見積もる。見えないパネルの描き直しを省く（カリング）のに使う。
    pub fn panel_seen(&self, panel: PanelId) -> bool {
        let p = self.overlay.panel(panel);
        !p.closed
            && self
                .subsystems
                .backend
                .overlay_seen(p.overlay_id())
                .unwrap_or_else(|| self.overlay.in_view(panel))
    }

    /// ランタイムが同時に出せるオーバーレイの数の上限（[`VrBackend::max_overlays`]）。
    pub fn max_overlays(&self) -> Option<usize> {
        self.subsystems.backend.max_overlays()
    }

    /// 最後の tick の HMD の姿勢。
    pub fn hmd(&self) -> Pose {
        self.overlay.hmd()
    }

    /// 最後の tick で骨格が取れていた手（スケルタル入力）。
    pub fn hands(&self) -> &[vr::HandSkeleton] {
        self.overlay.hands()
    }

    /// 再生中（一時停止中も含む）のメディア。何も再生していなければ `None`。読み込みは別スレッドで、最後に読んだもの。
    pub fn now_playing(&self) -> Option<std::sync::Arc<subsystems::media::NowPlaying>> {
        self.media.as_ref().and_then(|m| m.now())
    }

    /// 起動してから届いた OS の通知（通知センターに残っているもの。届いた順）。読み込みは別スレッドで、最後に読んだもの。
    pub fn notifications(&self) -> Arc<[Arc<subsystems::notifications::Notification>]> {
        self.notifications
            .as_ref()
            .map_or_else(|| Arc::from([]), |n| n.now())
    }

    /// OS の通知の監視を差し替える（`boot` は OS の監視を初期化する。E2E は固定一覧 `Notifications::fixed`）。
    pub fn set_notifications(&mut self, notifications: subsystems::notifications::Notifications) {
        self.notifications = Some(notifications);
    }

    /// 両目に映る範囲（ランタイムが教えてくれなければ `None`）。頭に付けて視野の端に出すもの（HUD）は、
    /// これを [`vr::Fov::safe`] に通した範囲に置く。
    pub fn fov(&self) -> Option<vr::Fov> {
        self.subsystems.backend.fov()
    }

    /// 最後の tick で電池の残量を申告していたデバイス。
    pub fn batteries(&self) -> &[vr::Battery] {
        &self.batteries
    }

    /// `panel` そのものをレーザーで指している手（[`Overlay::pointing_hands`]）。
    pub fn pointing_hands(&self, panel: PanelId) -> Vec<Hand> {
        self.overlay.pointing_hands(panel)
    }

    /// 掴んで取り外した（`false`）・ドッキングした（`true`）画面（[`Overlay::take_dock_changes`]）。受け取ると空になる。
    pub fn take_dock_changes(&mut self) -> Vec<(PanelId, bool)> {
        self.overlay.take_dock_changes()
    }

    /// 掴んだまま、はまる所から抜けた（`false`）・吸い付いた（`true`）画面（[`Overlay::take_dock_snaps`]）。受け取ると空になる。
    pub fn take_dock_snaps(&mut self) -> Vec<(PanelId, bool)> {
        self.overlay.take_dock_snaps()
    }

    /// 最後の tick の手ごとの入力の状態（骨格だけの手は判定に使った値 `metrics` 付き）。
    pub fn controllers(&self) -> &[ControllerState] {
        self.overlay.controllers()
    }

    /// `hand` のツールチップをカーソルの上下どちらに出すか（`Overlay::tooltip_anchor`）。
    pub fn tooltip_anchor(&self, hand: Hand) -> Anchor {
        self.overlay.tooltip_anchor(hand)
    }

    /// `hand` でドラッグしている間、その見た目を幅 `width_m` で出す（`Overlay::set_drag`）。`None` で隠す。
    pub fn set_drag(&mut self, hand: Hand, width_m: Option<f32>) {
        self.overlay.set_drag(hand, width_m)
    }

    /// ランタイムが終了を求めている（SteamVR の終了など）。呼び出し側はループを抜ける。
    pub fn quit_requested(&self) -> bool {
        self.overlay.quit_requested()
    }

    /// パネルの描画結果（RGBA8）をランタイムへ送る。
    pub fn submit(&mut self, panel: PanelId, width: u32, height: u32, rgba: &[u8]) {
        if self.desktop_streaming(panel) {
            return;
        }
        self.overlay
            .submit(&mut self.subsystems.backend, panel, width, height, rgba)
    }

    /// システムへの要求を処理する（[`Request`]）。メディアと通知の操作は各ワーカーへ送信するだけで待機しない。
    pub fn request(&mut self, request: Request) {
        match request {
            Request::Media(command) => {
                if let Some(media) = &self.media {
                    media.send(command);
                }
            }
            Request::Notification { id, action } => {
                if let Some(n) = &self.notifications {
                    n.activate(id, action);
                }
            }
            Request::SendNotification(toast) => {
                if let Some(n) = &self.notifications {
                    n.send(toast);
                }
            }
            Request::RemoveNotification(id) => {
                if let Some(n) = &self.notifications {
                    n.remove(id);
                }
            }
        }
    }

    pub fn apply(&mut self, panel: PanelId, cmd: PanelCommand) {
        match cmd {
            PanelCommand::Attach(device, offset) => self.overlay.attach(panel, device, offset),
            PanelCommand::Detach => self.overlay.detach(panel),
            PanelCommand::AttachToHand(hand) => {
                self.overlay
                    .attach_to_hand(&mut self.subsystems.backend, panel, hand)
            }
            PanelCommand::Lock(locked) => {
                self.overlay
                    .set_locked(&mut self.subsystems.backend, panel, locked)
            }
            PanelCommand::LazyFollow(follow) => self.overlay.set_lazy_follow(panel, follow),
            PanelCommand::Animated(animated) => {
                self.overlay
                    .set_animated(&mut self.subsystems.backend, panel, animated)
            }
            PanelCommand::Veil(veil) => self.overlay.set_veil(panel, veil),
            PanelCommand::Dim(dim) => self.overlay.set_dimmed(panel, dim),
            PanelCommand::PassThrough(pass) => self.overlay.set_pass_through(panel, pass),
            PanelCommand::FadeOnly(fade_only) => self.overlay.set_fade_only(panel, fade_only),
            PanelCommand::DockSlot(tilt) => self.overlay.set_dock_slot(panel, tilt),
            PanelCommand::Dockable(dockable) => self.overlay.set_dockable(panel, dockable),
            PanelCommand::Resizable(resizable) => self.overlay.set_resizable(panel, resizable),
            PanelCommand::CornerResize(limits) => self.overlay.set_corner_resize(panel, limits),
            PanelCommand::Reach(reach) => self.overlay.set_reach(panel, reach),
            PanelCommand::PointerMode(mode) => {
                self.pointer_modes.insert(panel, mode);
            }
            PanelCommand::GrabAnchor(anchor) => self.overlay.set_grab_anchor(panel, anchor),
            PanelCommand::MoveGrabAnchor(anchor) => self.overlay.move_grab_anchor(panel, anchor),
            PanelCommand::Hide(hidden) => {
                self.overlay
                    .set_hidden(&mut self.subsystems.backend, panel, hidden)
            }
            PanelCommand::Lockdown(on) => {
                if on {
                    self.lockdown_holders.insert(panel);
                } else {
                    self.lockdown_holders.remove(&panel);
                }
            }
            PanelCommand::Close if self.lockdown_holders.contains(&panel) => {
                logger::debug!(
                    "ignoring close of a panel holding the lockdown",
                    panel = panel
                );
            }
            PanelCommand::Close => {
                if let Some(sound) = self.close_sounds.remove(&panel) {
                    let (w, h) = self.overlay.panel(panel).resolution;
                    let center = Vec2::new(w as f32, h as f32) / 2.0;
                    self.play_audio_at(panel, sound, center, 1.0, 1.0);
                }
                self.overlay.close(&mut self.subsystems.backend, panel)
            }
            PanelCommand::Pointer(hand, part) => {
                self.overlay
                    .set_pointer(&mut self.subsystems.backend, panel, hand, part)
            }
            PanelCommand::Resize(w, h) => {
                self.overlay
                    .set_resolution(&mut self.subsystems.backend, panel, (w, h))
            }
            PanelCommand::FitHeight(height) => {
                self.overlay
                    .fit_height(&mut self.subsystems.backend, panel, height)
            }
            PanelCommand::Parent(parent) => {
                self.overlay
                    .set_parent(&mut self.subsystems.backend, panel, parent)
            }
            PanelCommand::NavBar(owner) => {
                self.overlay
                    .set_nav_bar(&mut self.subsystems.backend, panel, owner)
            }
            PanelCommand::Place(offset) => {
                self.overlay.detach(panel);
                self.overlay.place(panel, self.hmd().leveled() * offset);
            }
            PanelCommand::Stream(source, frame_rate) => {
                self.start_desktop(panel, source, frame_rate)
            }
            PanelCommand::FocusSource => self.focus_desktop(panel),
            PanelCommand::MinimizeSource => self.minimize_desktop(panel),
            PanelCommand::Play(sound, looping) => self.play_audio(panel, sound, looping),
            PanelCommand::PlayAt {
                sound,
                at,
                pitch,
                volume,
            } => self.play_audio_at(panel, sound, at, pitch, volume),
            PanelCommand::CloseSound(sound) => {
                self.close_sounds.insert(panel, sound);
            }
            PanelCommand::StopAudio => self.stop_audio(panel),
            PanelCommand::Spatial(spatial) => {
                self.configure_speaker(panel, |c| c.spatial = spatial)
            }
            PanelCommand::SpeakerLayout(layout) => {
                self.configure_speaker(panel, |c| c.layout = layout)
            }
            PanelCommand::TypeToOs { stroke, layout } => self.type_to_os(panel, stroke, layout),
        }
    }

    pub fn status(&self, panel: PanelId) -> PanelStatus {
        let p = self.overlay.panel(panel);
        PanelStatus {
            runtime: Some(self.subsystems.backend.runtime()),
            attached: p.attached.as_ref().map(|(d, _)| d.clone()),
            locked: p.locked,
            animated: p.animated,
            trackers: self.overlay.trackers().to_vec(),
            menu: self.overlay.menu_held(),
            double_press: self.shortcut.count
                + self.hand_shortcut.count
                + self.runtime_double_press,
            outside_press: self.overlay.outside_presses(),
            grabbed: self.overlay.grabbed(panel),
            closed: p.closed,
            hidden: p.hidden,
            lod: self.panel_lod(panel),
            parent: p.parent.map(|q| q.panel),
            pointed: self.overlay.pointed(panel),
            gazed: self.overlay.gazed(panel),
            desktop: self.desktop_panels.get(&panel).map(|p| p.status.clone()),
            desktop_stats: self.desktop_panels.get(&panel).and_then(|p| p.stats),
            audio: self.audio_status(panel),
            dock_hint: p
                .pointer
                .filter(|&(_, part)| part == Pointer::Dock)
                .and_then(|(hand, _)| self.overlay.dock_hint(hand)),
            corner_held: p.pointer.is_some_and(|(hand, part)| {
                part == Pointer::Corner && self.overlay.corner_held(hand)
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::mock::MockBackend;

    fn kernel() -> (Kernel<MockBackend>, PanelId) {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        let p = k.add_panel("t", Pose::IDENTITY, Vec2::ONE, (4, 4)).unwrap();
        (k, p)
    }

    #[test]
    fn double_press_counts_pairs_within_the_window() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut d = DoublePress::default();
        // 押しっぱなしは一回。
        for (held, at) in [(true, 0), (true, 50), (false, 100), (true, 200)] {
            d.update(held, ms(at));
        }
        assert_eq!(d.count, 1);
        // 三回目は数え直しの一回目。間が空きすぎた二回目も一回目になる。
        d.update(false, ms(250));
        d.update(true, ms(300));
        d.update(false, ms(350));
        let late = ms(300) + Duration::from_millis(251);
        d.update(true, late);
        assert_eq!(d.count, 1);
        d.update(false, late + Duration::from_millis(50));
        d.update(true, late + Duration::from_millis(250));
        assert_eq!(d.count, 2);
    }

    /// 手の長押しは 0.5 秒で一回。押し続けても増えず、途中で離すと数え直し。
    #[test]
    fn long_press_counts_once_per_hold() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut l = LongPress::default();
        for (held, at) in [
            (true, 0),
            (true, 300),
            (false, 400),
            (true, 500),
            (true, 900),
        ] {
            l.update(held, ms(at));
        }
        assert_eq!(l.count, 0, "離したので 0.5 秒続いていない");
        l.update(true, ms(1000));
        assert_eq!(l.count, 1);
        l.update(true, ms(3000));
        assert_eq!(l.count, 1, "押し続けても一回");
        l.update(false, ms(3100));
        l.update(true, ms(3200));
        l.update(true, ms(3700));
        assert_eq!(l.count, 2);
    }

    /// 手の調整は範囲に収めて入力層へ渡り、設定として読める。
    #[test]
    fn hand_setting_reaches_the_input_layer() {
        let (mut k, _) = kernel();
        let h = subsystems::input::HandTuning {
            pinch_mm: 1000.0,
            grip_deg: 75.0,
            ..Default::default()
        };
        k.set(Setting::Hand(h));
        let expected = subsystems::input::HandTuning {
            pinch_mm: subsystems::input::HandTuning::PINCH_MM.1,
            ..h
        };
        assert_eq!(k.settings().hand, expected);
        assert_eq!(k.subsystems.hand, expected);
        assert_eq!(k.config.input.hand, expected);
    }

    /// 左手の X ボタンを二回押すと、どのパネルの状態でも数が増える。
    #[test]
    fn x_pressed_twice_shows_in_status() {
        let (mut k, p) = kernel();
        let x = |k: &mut Kernel<MockBackend>, held: bool| {
            k.subsystems.backend.frame.controllers = vec![vr::RawController {
                hand: Hand::Left,
                profile: "oculus/touch_controller".into(),
                model: String::new(),
                aim: Pose::IDENTITY,
                inputs: vec![("x/click".into(), if held { 1.0 } else { 0.0 })],
            }];
            k.tick();
        };
        for held in [true, false, true, false] {
            x(&mut k, held);
        }
        assert_eq!(k.status(p).double_press, 1);
    }

    /// 利き手を替えても、カーネルは手に付いたパネルを動かさない（従うかは画面が決める）。FPS は 1〜240 に収める。
    #[test]
    fn dominant_hand_leaves_attached_panels_and_frame_rate_clamps() {
        let (mut k, p) = kernel();
        let offset = Pose::new(vr::Vec3::new(0.0, 0.1, 0.0), vr::Quat::IDENTITY);
        k.apply(
            p,
            PanelCommand::Attach(Device::Controller(Hand::Left), offset),
        );
        k.set(Setting::DominantHand(DominantHand::Left));
        assert_eq!(
            k.panel(p).attached,
            Some((Device::Controller(Hand::Left), offset))
        );
        assert_eq!(k.settings().dominant_hand, DominantHand::Left);
        k.set(Setting::WristDesign(WristDesign::Lcd));
        assert_eq!(k.settings().wrist_design, WristDesign::Lcd);
        assert_eq!(k.settings().update_channel, UpdateChannel::Stable);
        k.set(Setting::UpdateChannel(UpdateChannel::Nightly));
        assert_eq!(k.settings().update_channel, UpdateChannel::Nightly);
        k.set(Setting::MirrorFrameRate(1000));
        assert_eq!(k.settings().mirror_frame_rate, desktop::MAX_FRAME_RATE);
        k.set(Setting::MirrorFrameRate(0));
        assert_eq!(k.settings().mirror_frame_rate, 1);
    }

    /// ランタイムが数えた二回押し（SteamVR のダブルタップ）は、押下の間隔を見ずにそのまま一回と数える。
    #[test]
    fn runtime_double_press_counts_once_per_pulse() {
        let (mut k, p) = kernel();
        for pulse in [1.0, 1.0, 0.0] {
            k.subsystems.backend.frame.controllers = vec![vr::RawController {
                hand: Hand::Left,
                profile: "valve/index_controller".into(),
                model: String::new(),
                aim: Pose::IDENTITY,
                inputs: vec![(vr::DOUBLE_PRESS_INPUT.into(), pulse)],
            }];
            k.tick();
        }
        // 1 フレームだけ立つ入力なので、実際には 2 回目の 1.0 は別のダブルタップ。
        assert_eq!(k.status(p).double_press, 2);
    }

    #[cfg(feature = "novr")]
    #[test]
    fn boot_novr_by_name() {
        for name in ["novr", "--novr", "--no-vr", "NoVr", "no_vr"] {
            for gpu in [false, true] {
                let mut config = Config::default();
                config.vr.emulator_gpu = gpu;
                let k = Kernel::boot(Some(name), config, false).unwrap();
                assert_eq!(k.runtime(), Runtime::NoVr);
            }
        }
    }

    #[test]
    fn boot_rejects_unknown_runtime_instead_of_connecting_to_vr() {
        assert!(Kernel::boot(Some("--nvr"), Config::default(), true).is_err());
    }

    #[cfg(feature = "novr")]
    #[test]
    fn boot_without_request_always_succeeds() {
        // 実ランタイムの有無に依らず、指定なしなら novr にフォールバックして起動できる
        assert!(Kernel::boot(None, Config::default(), true).is_ok());
    }

    #[test]
    fn apply_lock_shows_in_status() {
        let (mut k, p) = kernel();
        assert!(!k.status(p).locked);
        k.apply(p, PanelCommand::Lock(true));
        assert!(k.status(p).locked);
    }

    #[test]
    fn place_detaches_and_moves_relative_to_head() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Attach(Device::Hmd, Pose::IDENTITY));
        let to = Pose::new(vr::Vec3::new(1.0, 0.0, -2.0), vr::Quat::IDENTITY);
        k.apply(p, PanelCommand::Place(to));
        assert_eq!(k.status(p).attached, None);
        assert_eq!(k.panel(p).pose, k.hmd().leveled() * to);
    }

    #[test]
    fn place_comes_in_front_of_the_face_upright_and_lying() {
        let (mut k, p) = kernel();
        let to = Pose::new(vr::Vec3::new(0.0, 0.0, -0.8), vr::Quat::IDENTITY);
        let head = vr::Vec3::new(0.2, 1.6, 0.1);
        // 立っていて首を少し傾けている: 正面の目の高さに、まっすぐ立てて出る。
        // 横向き・仰向けに寝転がっている: 顔の正面に、頭の上を上にして出る。
        for (q, upright) in [
            (
                vr::Quat::from_rotation_y(0.8) * vr::Quat::from_rotation_z(0.3),
                true,
            ),
            (
                vr::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                false,
            ),
            (
                vr::Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                false,
            ),
        ] {
            k.subsystems.backend.frame.hmd = Pose::new(head, q);
            k.tick();
            k.apply(p, PanelCommand::Place(to));
            let pose = k.panel(p).pose;
            let ahead = head + q * vr::Vec3::new(0.0, 0.0, -0.8);
            let up = pose.orientation * vr::Vec3::Y;
            if upright {
                assert!((pose.position.y - head.y).abs() < 1e-5, "{q}");
                assert!(up.abs_diff_eq(vr::Vec3::Y, 1e-5), "{q}");
            } else {
                assert!(
                    pose.position.abs_diff_eq(ahead, 1e-5),
                    "{q} {}",
                    pose.position
                );
                assert!(up.abs_diff_eq(q * vr::Vec3::Y, 1e-5), "{q}");
            }
            // どちらでも表面は頭を向く。
            let to_head = (head - pose.position).normalize();
            assert!((pose.orientation * vr::Vec3::Z).dot(to_head) > 0.99, "{q}");
        }
    }

    /// 境目の前後で行き来しない: 上げるのは閾値で、下げるのは閾値の 9 割を割ってから。
    #[test]
    fn lockdown_protects_only_its_holders_and_lasts_until_all_release() {
        let (mut k, _) = lod_kernel();
        let mut panel = |name| {
            k.add_panel(name, Pose::IDENTITY, Vec2::new(1.0, 1.0), (100, 100))
                .unwrap()
        };
        let (a, b, other) = (panel("a"), panel("b"), panel("other"));
        assert!(!k.lockdown());
        k.apply(a, PanelCommand::Lockdown(true));
        k.apply(b, PanelCommand::Lockdown(true));
        assert!(k.settings().lockdown);
        // 取っているパネルだけが閉じられない。ほかは閉じられる。
        k.apply(a, PanelCommand::Close);
        assert!(!k.panel(a).closed);
        k.apply(other, PanelCommand::Close);
        assert!(k.panel(other).closed);
        // 1 枚放しても、もう 1 枚が取っている間は続く。放したパネルは閉じられる。
        k.apply(a, PanelCommand::Lockdown(false));
        assert!(k.lockdown());
        k.apply(a, PanelCommand::Close);
        assert!(k.panel(a).closed);
        k.apply(b, PanelCommand::Lockdown(false));
        assert!(!k.lockdown());
    }

    #[test]
    fn lockdown_is_released_then_the_holder_closes() {
        let (mut k, _) = lod_kernel();
        let p = k
            .add_panel("p", Pose::IDENTITY, Vec2::new(1.0, 1.0), (100, 100))
            .unwrap();
        k.apply(p, PanelCommand::Lockdown(true));
        assert!(k.lockdown());
        // 放してから閉じる（同じパネルのコマンド列の順）。
        for cmd in [PanelCommand::Lockdown(false), PanelCommand::Close] {
            k.apply(p, cmd);
        }
        assert!(k.panel(p).closed && !k.lockdown());
    }

    #[test]
    fn setup_done_starts_false_and_is_reflected() {
        let (mut k, _) = lod_kernel();
        assert!(!k.settings().setup_done);
        k.set(Setting::SetupDone(true));
        assert!(k.settings().setup_done);
    }

    #[test]
    fn lod_quality_setting_is_reflected() {
        let (mut k, _) = lod_kernel();
        k.set(Setting::LodQuality(LodQuality::High));
        assert_eq!(k.settings().lod_quality, LodQuality::High);
    }

    /// 自動起動はランタイムが持つ値をそのまま見せ、切り替えられないランタイムでは `None` のまま。
    #[test]
    fn auto_launch_follows_the_runtime() {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        k.set(Setting::AutoLaunch(true));
        assert_eq!(k.settings().auto_launch, None);
        k.subsystems.backend.auto_launch = Some(false);
        k.set(Setting::AutoLaunch(true));
        assert_eq!(k.settings().auto_launch, Some(true));
    }

    /// キーの音は既定で鳴り、音量は最大。オン・オフと音量は独立で、音量は 0〜1 の刻みに収まる。
    #[test]
    fn key_sound_is_on_at_full_volume_by_default_and_the_volume_stays_in_range() {
        let (mut k, _) = lod_kernel();
        let (s, d) = (k.settings(), Settings::default());
        assert_eq!((s.key_sound, s.key_sound_volume), (true, 1.0));
        assert_eq!((d.key_sound, d.key_sound_volume), (true, 1.0));
        k.set(Setting::KeySoundVolume(0.44));
        let s = k.settings();
        assert!((s.key_sound_volume - 0.4).abs() < 1e-6 && s.key_sound);
        k.set(Setting::KeySound(false));
        let s = k.settings();
        assert!((s.key_sound_volume - 0.4).abs() < 1e-6 && !s.key_sound);
        for (given, kept) in [(9.0, 1.0), (-2.0, 0.0), (f32::NAN, 1.0)] {
            k.set(Setting::KeySoundVolume(given));
            assert_eq!(k.settings().key_sound_volume, kept, "{given}");
        }
        // 設定ファイルの値も同じく収める。
        let mut config = Config::default();
        config.keyboard.key_sound = Some(false);
        config.keyboard.key_sound_volume = Some(3.0);
        let k = Kernel::with_backend(vr::mock::MockBackend::default(), config);
        let s = k.settings();
        assert_eq!((s.key_sound, s.key_sound_volume), (false, 1.0));
    }

    /// キーボードの大きさとキーの間隔は既定で等倍。互いに独立で、範囲と刻みに収まり、設定ファイルの値も同じく収める。
    #[test]
    fn keyboard_scale_and_key_gap_are_one_by_default_and_stay_in_range() {
        let (mut k, _) = lod_kernel();
        let (s, d) = (k.settings(), Settings::default());
        assert_eq!((s.keyboard_scale, s.key_gap), (1.0, 1.0));
        assert_eq!((d.keyboard_scale, d.key_gap), (1.0, 1.0));
        k.set(Setting::KeyboardScale(1.26));
        let s = k.settings();
        assert!((s.keyboard_scale - 1.3).abs() < 1e-6 && s.key_gap == 1.0);
        k.set(Setting::KeyGap(0.44));
        let s = k.settings();
        assert!((s.keyboard_scale - 1.3).abs() < 1e-6 && (s.key_gap - 0.4).abs() < 1e-6);
        for (given, scale, gap) in [
            (9.0, KEYBOARD_SCALE_MAX, KEY_GAP_MAX),
            (-1.0, KEYBOARD_SCALE_MIN, KEY_GAP_MIN),
            (f32::NAN, 1.0, 1.0),
        ] {
            k.set(Setting::KeyboardScale(given));
            k.set(Setting::KeyGap(given));
            let s = k.settings();
            assert_eq!((s.keyboard_scale, s.key_gap), (scale, gap), "{given}");
        }
        let mut config = Config::default();
        config.keyboard.scale = Some(7.0);
        config.keyboard.key_gap = Some(-3.0);
        let k = Kernel::with_backend(vr::mock::MockBackend::default(), config);
        let s = k.settings();
        assert_eq!(
            (s.keyboard_scale, s.key_gap),
            (KEYBOARD_SCALE_MAX, KEY_GAP_MIN)
        );
    }

    /// wrist の姿勢は置き直すまで無く、大きさは等倍。置き直した姿勢はそのまま、大きさは刻まずに範囲に収めて覚える。
    #[test]
    fn wrist_placement_is_remembered_and_scale_stays_in_range() {
        let (mut k, _) = lod_kernel();
        let (s, d) = (k.settings(), Settings::default());
        assert_eq!((s.wrist_offset, s.wrist_scale), (None, 1.0));
        assert_eq!((d.wrist_offset, d.wrist_scale), (None, 1.0));
        let pose = Pose::new(
            vr::Vec3::new(-0.1, 0.02, 0.15),
            vr::Quat::from_rotation_x(0.3),
        );
        k.set(Setting::WristOffset(pose));
        k.set(Setting::WristScale(1.37));
        let s = k.settings();
        assert_eq!(s.wrist_offset.unwrap().position, pose.position);
        assert!(
            s.wrist_offset
                .unwrap()
                .orientation
                .abs_diff_eq(pose.orientation, 1e-6)
        );
        assert_eq!(s.wrist_scale, 1.37, "掴んで変えた大きさは刻まない");
        for (given, scale) in [
            (9.0, WRIST_SCALE_MAX),
            (0.0, WRIST_SCALE_MIN),
            (f32::NAN, 1.0),
        ] {
            k.set(Setting::WristScale(given));
            assert_eq!(k.settings().wrist_scale, scale, "{given}");
        }
    }

    /// ダッシュボードの距離・高さは既定で正面 1.3m・高さ 0（目の高さの 0.05m 下）。互いに独立で、範囲と刻みに収まる。
    #[test]
    fn dashboard_placement_has_defaults_and_stays_in_range() {
        let (mut k, _) = lod_kernel();
        let (s, d) = (k.settings(), Settings::default());
        assert_eq!((s.dashboard_distance, s.dashboard_height), (1.3, 0.0));
        assert_eq!((d.dashboard_distance, d.dashboard_height), (1.3, 0.0));
        k.set(Setting::DashboardDistance(1.52));
        k.set(Setting::DashboardHeight(-0.18));
        let s = k.settings();
        assert!((s.dashboard_distance - 1.5).abs() < 1e-6, "{s:?}");
        assert!((s.dashboard_height + 0.2).abs() < 1e-6, "{s:?}");
        for (given, kept) in [(9.0, DASHBOARD_DISTANCE_MAX), (0.0, DASHBOARD_DISTANCE_MIN)] {
            k.set(Setting::DashboardDistance(given));
            assert_eq!(k.settings().dashboard_distance, kept, "{given}");
        }
        k.set(Setting::DashboardHeight(f32::NAN));
        assert_eq!(k.settings().dashboard_height, 0.0);
        // 設定ファイルの値も同じく収める。
        let mut config = Config::default();
        config.dashboard.distance = Some(-1.0);
        config.dashboard.height = Some(3.0);
        let s = Kernel::with_backend(vr::mock::MockBackend::default(), config).settings();
        assert_eq!(
            (s.dashboard_distance, s.dashboard_height),
            (DASHBOARD_DISTANCE_MIN, DASHBOARD_HEIGHT_MAX)
        );
    }

    /// ダッシュボードの背景の暗さの既定値は `overlay::VEIL`。設定した値は 0〜1 の範囲と刻みに収めたうえで、暗転の幕の
    /// 濃さになる。設定ファイルから読んだ値も同様に収める。
    #[test]
    fn dashboard_dim_drives_the_veil_and_stays_in_range() {
        let (mut k, _) = lod_kernel();
        let (s, d) = (k.settings(), Settings::default());
        assert_eq!(
            (s.dashboard_dim, d.dashboard_dim),
            (overlay::VEIL, overlay::VEIL)
        );
        k.set(Setting::DashboardDim(0.26));
        assert!((k.settings().dashboard_dim - 0.3).abs() < 1e-6);
        assert_eq!(k.overlay.veil_opacity(), k.settings().dashboard_dim);
        assert_eq!(
            k.config.overlay.veil_opacity,
            Some(k.settings().dashboard_dim)
        );
        for (given, kept) in [(9.0, 1.0), (-1.0, 0.0), (f32::NAN, overlay::VEIL)] {
            k.set(Setting::DashboardDim(given));
            assert_eq!(k.settings().dashboard_dim, kept, "{given}");
        }
        let mut config = Config::default();
        config.overlay.veil_opacity = Some(-3.0);
        let k = Kernel::with_backend(vr::mock::MockBackend::default(), config);
        assert_eq!(k.overlay.veil_opacity(), 0.0);
    }

    /// ポインタの画面の設定: 既定はシンプル・等倍・どれもオン。カーソルの大きさは範囲と刻みに収まり、設定ファイルの値も同じく収める。
    #[test]
    fn pointer_settings_default_on_and_cursor_scale_stays_in_range() {
        let (mut k, _) = lod_kernel();
        let (s, d) = (k.settings(), Settings::default());
        for s in [s, d] {
            assert_eq!(s.pointer_tuning, PointerTuning::Simple);
            assert_eq!(
                (s.cursor_scale, s.tooltips, s.tap_sound, s.hover_sound),
                (1.0, true, true, true)
            );
        }
        k.set(Setting::PointerTuning(PointerTuning::Detailed));
        k.set(Setting::Tooltips(false));
        k.set(Setting::TapSound(false));
        let s = k.settings();
        assert_eq!(s.pointer_tuning, PointerTuning::Detailed);
        assert_eq!(
            (s.tooltips, s.tap_sound, s.hover_sound),
            (false, false, true)
        );
        k.set(Setting::CursorScale(1.44));
        assert!((k.settings().cursor_scale - 1.4).abs() < 1e-6);
        for (given, kept) in [
            (9.0, CURSOR_SCALE_MAX),
            (0.0, CURSOR_SCALE_MIN),
            (f32::NAN, 1.0),
        ] {
            k.set(Setting::CursorScale(given));
            assert!((k.settings().cursor_scale - kept).abs() < 1e-6, "{given}");
        }
        let mut config = Config::default();
        config.overlay.cursor_scale = Some(7.0);
        config.ui.hover_sound = Some(false);
        let s = Kernel::with_backend(MockBackend::default(), config).settings();
        assert_eq!((s.cursor_scale, s.hover_sound), (CURSOR_SCALE_MAX, false));
    }

    /// アクセシビリティの 3 つは独立した設定: どれかを変えてもほかは変わらず、どんな値の組でも範囲に収まる。
    #[test]
    fn accessibility_settings_are_independent_and_always_in_range() {
        let (mut k, p) = lod_kernel();
        let s = k.settings();
        assert_eq!((s.text_scale, s.ui_scale, s.bold_text), (1.0, 1.0, false));
        k.set(Setting::TextScale(1.3));
        let s = k.settings();
        assert!((s.text_scale - 1.3).abs() < 1e-6);
        assert_eq!((s.ui_scale, s.bold_text), (1.0, false));
        assert_eq!(
            k.panel(p).size,
            Vec2::ONE,
            "文字の大きさではパネルは変わらない"
        );
        k.set(Setting::BoldText(true));
        k.set(Setting::UiScale(1.5));
        let s = k.settings();
        assert!((s.text_scale - 1.3).abs() < 1e-6 && s.bold_text);
        assert_eq!((s.ui_scale, k.ui_scale()), (1.5, 1.5));
        // UI スケールは開いているパネルの実寸をその場で変え、解像度（中の組み方）は変えない。後から開くパネルにも掛かる。
        assert_eq!(
            (k.panel(p).size, k.panel(p).resolution),
            (Vec2::splat(1.5), (1000, 1000))
        );
        let later = k
            .add_panel("later", Pose::IDENTITY, Vec2::new(0.8, 0.6), (800, 600))
            .unwrap();
        assert!(k.panel(later).size.abs_diff_eq(Vec2::new(1.2, 0.9), 1e-6));
        // 端から端まで、数でない値も含めてどの組でも範囲の中。
        let wild = [
            f32::NEG_INFINITY,
            -1.0,
            0.0,
            0.05,
            1.0,
            99.0,
            f32::INFINITY,
            f32::NAN,
        ];
        for text in wild {
            for ui in wild {
                k.set(Setting::TextScale(text));
                k.set(Setting::UiScale(ui));
                let s = k.settings();
                assert!(
                    (TEXT_SCALE_MIN..=TEXT_SCALE_MAX).contains(&s.text_scale),
                    "{text}"
                );
                assert!((UI_SCALE_MIN..=UI_SCALE_MAX).contains(&s.ui_scale), "{ui}");
                let size = k.panel(p).size;
                assert!(
                    size.abs_diff_eq(Vec2::splat(s.ui_scale), 1e-4),
                    "{ui}: {size}"
                );
            }
        }
    }

    /// 起動時の設定も範囲に収めて効く（UI スケールは最初のパネルから掛かる）。
    #[test]
    fn accessibility_config_applies_at_boot() {
        let mut config = Config::default();
        config.design.text_scale = Some(7.0);
        config.overlay.ui_scale = Some(0.1);
        config.design.bold_text = true;
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        let s = k.settings();
        assert_eq!(
            (s.text_scale, s.ui_scale, s.bold_text),
            (TEXT_SCALE_MAX, UI_SCALE_MIN, true)
        );
        let p = k
            .add_panel("t", Pose::IDENTITY, Vec2::ONE, (10, 10))
            .unwrap();
        assert!(k.panel(p).size.abs_diff_eq(Vec2::splat(UI_SCALE_MIN), 1e-6));
    }

    #[test]
    fn lod_does_not_flap_at_the_threshold() {
        let (mut k, p) = lod_kernel();
        k.config.kernel.lod_quality = LodQuality::Low;
        let pose = k.panel(p).pose;
        for (distance, lod) in [(1.0, 1), (0.95, 1), (1.0, 1), (0.85, 0), (0.95, 0)] {
            k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * distance;
            k.tick();
            assert_eq!(k.panel_lod(p), lod, "{distance}");
        }
    }

    /// 基準の密度（[`LOD_PX_PER_M`]）のパネルを 1 枚。
    fn lod_kernel() -> (Kernel<MockBackend>, PanelId) {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        let p = k
            .add_panel("t", Pose::IDENTITY, Vec2::ONE, (1000, 1000))
            .unwrap();
        (k, p)
    }

    /// Issue #51: 遠くで LOD が効いたまま拡大すると粗いまま残っていた。拡大して密度が下がれば、同じ距離でも LOD は下がる。
    #[test]
    fn scaled_up_panel_gets_a_finer_lod_at_the_same_distance() {
        let (mut k, p) = lod_kernel();
        k.config.kernel.lod_quality = LodQuality::Low;
        // 同じ解像度のまま 4 倍に拡大したパネル（密度 1/4）を同じ所に。
        let big = k
            .add_panel("big", Pose::IDENTITY, Vec2::splat(4.0), (1000, 1000))
            .unwrap();
        k.subsystems.backend.frame.hmd.position = vr::Vec3::Z * 3.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 2);
        assert_eq!(k.panel_lod(big), 0, "3m / 4 = 0.75m 相当");
    }

    #[test]
    fn tooltip_keeps_full_resolution_when_far_from_the_head() {
        let (mut k, p) = lod_kernel();
        k.apply(p, PanelCommand::Pointer(vr::Hand::Right, Pointer::Tooltip));
        k.subsystems.backend.frame.hmd.position = vr::Vec3::Z * 100.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 0);
    }

    /// はまる所の見本の帯の文字が、距離で粗くならない。
    #[test]
    fn dock_ghost_keeps_full_resolution_when_far_from_the_head() {
        let (mut k, p) = lod_kernel();
        k.apply(p, PanelCommand::Pointer(vr::Hand::Right, Pointer::Dock));
        k.subsystems.backend.frame.hmd.position = vr::Vec3::Z * 100.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 0);
    }

    /// 遠くのパネルでも、レーザーで指している間は LOD0（操作したいのに粗くて見づらい）。外せば距離の段階に戻る。
    #[test]
    fn pointed_panel_keeps_full_resolution_when_far_from_the_head() {
        let (mut k, p) = lod_kernel();
        k.subsystems.backend.frame.hmd.position = vr::Vec3::Z * 100.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 3);
        k.subsystems.backend.frame.controllers = vec![vr::RawController {
            hand: Hand::Right,
            profile: "khr/simple_controller".into(),
            model: String::new(),
            aim: Pose::new(vr::Vec3::Z, vr::Quat::IDENTITY),
            inputs: vec![],
        }];
        k.tick();
        assert!(k.status(p).pointed);
        assert_eq!(k.panel_lod(p), 0);
        k.subsystems.backend.frame.controllers.clear();
        k.tick();
        assert_eq!(k.panel_lod(p), 3);
    }

    #[test]
    fn distance_lod_tracks_camera_and_preserves_panel_geometry() {
        let (mut k, p) = lod_kernel();
        let pose = k.panel(p).pose;
        let resolution = k.panel(p).resolution;
        let size = k.panel(p).size;
        k.config.kernel.lod_quality = LodQuality::Off;
        k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * 100.0;
        k.tick();
        assert_eq!(k.panel_lod(p), 0);
        for (quality, near) in [
            (LodQuality::Low, 1.0),
            (LodQuality::Mid, 2.0),
            (LodQuality::High, 4.0),
        ] {
            k.config.kernel.lod_quality = quality;
            for (distance, lod) in [
                (0.0, 0),
                (near - 0.01, 0),
                (near, 1),
                (near * 2.0, 2),
                (near * 4.0, 3),
                (100.0, 3),
                (0.5, 0),
            ] {
                k.subsystems.backend.frame.hmd.position = pose.position + vr::Vec3::Z * distance;
                k.tick();
                assert_eq!(k.panel_lod(p), lod, "{quality:?}: {distance}");
                let due = (0..8)
                    .filter(|_| {
                        k.tick();
                        k.panel_frame_due(p)
                    })
                    .count();
                assert_eq!(due, 8 >> lod, "{quality:?}: {distance}");
                assert_eq!(k.panel(p).resolution, resolution);
                assert_eq!(k.panel(p).size, size);
            }
        }
    }

    #[test]
    fn apply_resize_changes_resolution_and_vr_width() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Resize(8, 2));
        assert_eq!(k.panel(p).resolution, (8, 2));
        assert_eq!(k.panel(p).size, Vec2::new(2.0, 0.5));
        assert_eq!(k.subsystems.backend.overlays[0].width_m, 2.0);
    }

    #[test]
    fn pointer_mode_starts_from_config_and_switches() {
        let mut config = Config::default();
        config.kernel.pointer_mode = Some(PointerMode::Touch);
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        assert_eq!(k.settings().pointer_mode, PointerMode::Touch);
        k.set(Setting::PointerMode(PointerMode::Mouse));
        assert_eq!(k.pointer_mode(), PointerMode::Mouse);
        assert_eq!(
            kernel().0.pointer_mode(),
            PointerMode::Touch,
            "defaults to touch"
        );
        // パネルごとに決めた方式は、全体の設定より優先する。
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::PointerMode(PointerMode::Mouse));
        assert_eq!(k.panel_pointer_mode(p), PointerMode::Mouse);
        assert_eq!(k.pointer_mode(), PointerMode::Touch);
    }

    #[test]
    fn theme_starts_from_config_and_switches_without_touching_files() {
        let mut config = Config::default();
        config.design.theme = ThemeMode::Light;
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        assert_eq!(k.settings().theme, ThemeMode::Light);
        k.set(Setting::Theme(ThemeMode::Dark));
        assert_eq!(k.settings().theme, ThemeMode::Dark);
    }

    #[test]
    fn language_starts_from_config_and_persists_to_config_path() {
        let mut config = Config::default();
        config.i18n.language = Some(Language::Ja);
        let mut k = Kernel::with_backend(MockBackend::default(), config);
        assert_eq!(k.settings().language, Language::Ja);
        let path =
            std::env::temp_dir().join(format!("nyaw-kernel-lang-{}.toml", std::process::id()));
        k.config_path = Some(path.clone());
        k.set(Setting::Language(Language::En));
        assert_eq!(k.settings().language, Language::En);
        let saved = config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(saved.i18n.language, Some(Language::En));
    }

    /// パーソナルカラーは選ぶまで `None`。範囲に収めて覚え、設定ファイルへ表ごと書き戻す。
    #[test]
    fn personal_color_is_clamped_and_persists() {
        let path =
            std::env::temp_dir().join(format!("nyaw-kernel-color-{}.toml", std::process::id()));
        let (mut k, _) = kernel();
        assert_eq!(k.settings().personal_color, None);
        k.config_path = Some(path.clone());
        k.set(Setting::PersonalColor(PersonalColor {
            lightness: 0.87,
            chroma: 2.0,
            hue: 370.0,
        }));
        let want = Some(PersonalColor {
            lightness: 0.87,
            chroma: 0.4,
            hue: 10.0,
        });
        assert_eq!(k.settings().personal_color, want);
        let saved = config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(saved.design.personal_color, want);
    }

    /// 自分で作った色は選ばれ、履歴の先頭に残る（同じ色は 1 つ、古いものから落ちる）。選ぶだけなら履歴は変わらない。
    #[test]
    fn custom_colors_are_selected_and_remembered_newest_first() {
        let path =
            std::env::temp_dir().join(format!("nyaw-kernel-history-{}.toml", std::process::id()));
        let (mut k, _) = kernel();
        k.config_path = Some(path.clone());
        let color = |hue: f32| PersonalColor {
            lightness: 0.6,
            chroma: 0.2,
            hue,
        };
        assert_eq!(k.settings().color_history, [None; COLOR_HISTORY]);
        k.set(Setting::PersonalColor(color(1.0)));
        assert_eq!(k.settings().color_history[0], None, "選ぶだけでは覚えない");
        for hue in [10.0, 20.0, 10.0] {
            k.set(Setting::CustomColor(color(hue)));
        }
        let s = k.settings();
        assert_eq!(s.personal_color, Some(color(10.0)));
        let newest = [Some(color(10.0)), Some(color(20.0)), None];
        assert_eq!(s.color_history[..3], newest);
        let saved = config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(saved.design.personal_color, Some(color(10.0)));
        assert_eq!(saved.design.color_history, [color(10.0), color(20.0)]);
        for n in 0..COLOR_HISTORY + 2 {
            k.set(Setting::CustomColor(color(100.0 + n as f32)));
        }
        let history = k.settings().color_history;
        assert_eq!(history[0], Some(color(101.0 + COLOR_HISTORY as f32)));
        let oldest = Some(color(102.0));
        assert_eq!(history[COLOR_HISTORY - 1], oldest, "古いものから落ちる");
    }

    #[test]
    fn theme_setting_persists_to_config_path() {
        let path = std::env::temp_dir().join(format!("nyaw-kernel-{}.toml", std::process::id()));
        let (mut k, _) = kernel();
        k.config_path = Some(path.clone());
        k.set(Setting::Theme(ThemeMode::Light));
        let saved = config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(saved.design.theme, ThemeMode::Light);
    }

    #[test]
    fn submit_reaches_backend() {
        let (mut k, p) = kernel();
        k.apply(p, PanelCommand::Animated(false));
        k.submit(p, 1, 1, &[1, 2, 3, 4]);
        let tex = k.subsystems.backend.overlays[0].texture.as_ref().unwrap();
        assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
    }

    #[test]
    fn animation_command_opts_out_only_the_selected_panel() {
        let (mut k, p) = kernel();
        let other = k
            .add_panel("other", Pose::IDENTITY, Vec2::ONE, (4, 4))
            .unwrap();
        k.apply(p, PanelCommand::Animated(false));
        assert!(!k.status(p).animated && k.status(other).animated);
        k.submit(p, 1, 1, &[255; 4]);
        k.submit(other, 1, 1, &[255; 4]);
        assert_eq!(k.subsystems.backend.overlays[p].width_m, 1.0);
        assert_eq!(k.subsystems.backend.overlays[other].width_m, 0.75);
        k.apply(p, PanelCommand::Close);
        assert!(k.status(p).closed && !k.subsystems.backend.overlays[p].visible);
        k.apply(other, PanelCommand::Animated(false));
        assert_eq!(
            k.subsystems.backend.overlays[other]
                .texture
                .as_ref()
                .unwrap()
                .pixel(0, 0),
            [255; 4]
        );
    }
}
