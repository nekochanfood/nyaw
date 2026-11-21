//! カーネル内の設定: 設定ファイル。`os::config_dir()/config.toml`（現在のバックエンドでは `%LOCALAPPDATA%\nyaw`。debug ビルドは `nyaw-dev`）を読む。
//! ファイルが無ければ既定値。優先順位（コマンドライン > 環境変数 > ここ）は呼び出し側が決める。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use subsystems::{os, vr};

/// 環境変数 `NYAW_LOG` で指定されたログレベル（`trace` / `debug` / `info` / `warn` / `error`）。解釈は呼び出し側。
pub fn log_level() -> Option<String> {
    std::env::var("NYAW_LOG").ok().filter(|v| !v.is_empty())
}

/// 距離 LOD の品質。0 は無効、数字が小さいほど近い距離で解像度を落とす。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub enum LodQuality {
    Off = 0,
    Low = 1,
    #[default]
    Mid = 2,
    High = 3,
}

impl Serialize for LodQuality {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(*self as u8)
    }
}

impl TryFrom<u8> for LodQuality {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Off),
            1 => Ok(Self::Low),
            2 => Ok(Self::Mid),
            3 => Ok(Self::High),
            _ => Err("lod_quality must be 0 (Off), 1 (Low), 2 (Mid), or 3 (High)"),
        }
    }
}

/// トリガーを押下したまま移動した際の動作。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerMode {
    /// マウスのエミュレート: 押下したまま移動するとドラッグ。
    Mouse,
    /// タッチのエミュレート（既定）: 押下したまま移動するとスワイプ（スクロール。離すと慣性スクロール）、移動せずに離すとタップ（クリック）、
    /// 移動せずに長押しするとマウスの押下（そのまま移動するとドラッグ）。
    #[default]
    Touch,
}

/// 設定画面のポインタの動きの調整方式。シンプルは 1 本のスライダー（手ブレ抑えの強さと緩和角度を連動して変更する）、
/// 詳細はそれぞれのスライダー。カーネルは値を保持するのみで、解釈は設定画面が行う。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerTuning {
    #[default]
    Simple,
    Detailed,
}

/// 見た目のテーマ（ライト / ダーク）。`design::Mode` に対応する。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

/// 画面の言語（`en` / `ja`）。`i18n::Lang` に対応する。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    Ja,
}

impl Language {
    /// ISO 639-1 の 2 文字（`os::ui_language`）から。対応していない言語は `None`。
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            "ja" => Some(Self::Ja),
            _ => None,
        }
    }
}

/// パーソナルカラー: ボタンなどの塗りに使用するユーザー定義の色（OKLCH）。`design::Accent` に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PersonalColor {
    /// 明るさ（0〜1）。
    pub lightness: f32,
    /// 鮮やかさ（0〜0.4。画面に表示可能な範囲を超える分は、描画時にクリッピングする）。
    pub chroma: f32,
    /// 色相（度。0〜360）。
    pub hue: f32,
}

impl PersonalColor {
    /// 範囲に収める（色相は 0〜360 にラップする。非数値は 0）。
    pub fn clamped(self) -> Self {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        Self {
            lightness: finite(self.lightness).clamp(0.0, 1.0),
            chroma: finite(self.chroma).clamp(0.0, 0.4),
            hue: finite(self.hue).rem_euclid(360.0),
        }
    }
}

/// 文字の大きさの倍率（[`DesignTable::text_scale`]）と UI スケール（[`OverlayTable::ui_scale`]）の範囲と刻み。画面内のレイアウトは
/// 変更しないため、文字はコンテナに収まる倍率まで。UI スケールはパネルごと拡大するため、視野に収まる倍率まで。
pub const TEXT_SCALE_MIN: f32 = 0.8;
pub const TEXT_SCALE_MAX: f32 = 1.5;
pub const UI_SCALE_MIN: f32 = 0.7;
pub const UI_SCALE_MAX: f32 = 1.5;
pub const SCALE_STEP: f32 = 0.1;
/// カーソルの大きさの倍率（[`OverlayTable::cursor_scale`]）の範囲。刻みは [`SCALE_STEP`]。
pub const CURSOR_SCALE_MIN: f32 = 0.5;
pub const CURSOR_SCALE_MAX: f32 = 2.0;
/// 仮想キーボードの大きさの倍率（[`KeyboardTable::scale`]）の範囲。刻みは [`SCALE_STEP`]。
pub const KEYBOARD_SCALE_MIN: f32 = 0.5;
pub const KEYBOARD_SCALE_MAX: f32 = 2.0;
/// 仮想キーボードのキーの間隔の倍率（[`KeyboardTable::key_gap`]。種類の既定の隙間に掛ける）の範囲。刻みは [`SCALE_STEP`]。
pub const KEY_GAP_MIN: f32 = 0.0;
pub const KEY_GAP_MAX: f32 = 2.0;

/// 倍率を範囲 `min..=max` に収め、[`SCALE_STEP`] の刻みに丸める（非数値は等倍）。
pub fn clamp_scale(scale: f32, min: f32, max: f32) -> f32 {
    snap(scale, min, max, SCALE_STEP, 1.0)
}

/// 値を範囲 `min..=max` に収め、`step` の刻みに丸める（数でない値は `fallback`。-0 は 0 にする）。
/// 刻みの数を `step` で掛けず `1 / step` で割るので、1.3 が 1.3000001 になるような誤差が出ない。
pub fn snap(value: f32, min: f32, max: f32, step: f32, fallback: f32) -> f32 {
    let value = if value.is_finite() { value } else { fallback };
    ((value.clamp(min, max) / step).round() / (1.0 / step)).clamp(min, max) + 0.0
}

/// ダッシュボードの置き場所（[`DashboardTable::distance`] / [`DashboardTable::height`]）の既定・範囲と刻み（m）。
/// 距離ははめた画面の中心まで。高さは目の高さの 0.1m 下（ダッシュボードの `HEIGHT_ORIGIN`）を 0 とする相対（上が +）。
pub const DASHBOARD_DISTANCE: f32 = 1.3;
pub const DASHBOARD_DISTANCE_MIN: f32 = 0.6;
pub const DASHBOARD_DISTANCE_MAX: f32 = 2.0;
pub const DASHBOARD_HEIGHT_MIN: f32 = -0.5;
pub const DASHBOARD_HEIGHT_MAX: f32 = 0.5;
pub const DASHBOARD_STEP: f32 = 0.05;

/// 自分で作ったパーソナルカラーを覚えておく数（[`DesignTable::color_history`]）。
pub const COLOR_HISTORY: usize = 8;

/// wrist の大きさの倍率（[`WristTable::scale`]）の範囲。掴んで変えた大きさなので刻みには丸めない。
pub const WRIST_SCALE_MIN: f32 = 0.5;
pub const WRIST_SCALE_MAX: f32 = 3.0;

/// wrist を掴んで置き直した姿勢（[`WristTable::offset`]）。取り付け先のコントローラの tip から見た、左手首での値
/// （右手首には wrist が鏡写しにして付ける）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WristOffset {
    /// 位置（m。`[x, y, z]`）。
    pub position: [f32; 3],
    /// 向き（四元数。`[x, y, z, w]`）。
    pub orientation: [f32; 4],
}

impl WristOffset {
    /// 姿勢にする。数でない値・長さ 0 の向きは壊れているので `None`。向きは長さ 1 に直す。
    pub fn pose(self) -> Option<vr::Pose> {
        let position = vr::Vec3::from_array(self.position);
        let orientation = vr::Quat::from_array(self.orientation);
        let ok = position.is_finite() && orientation.is_finite() && orientation.length() > 1e-6;
        ok.then(|| vr::Pose::new(position, orientation.normalize()))
    }
}

impl From<vr::Pose> for WristOffset {
    fn from(p: vr::Pose) -> Self {
        Self {
            position: p.position.to_array(),
            orientation: p.orientation.to_array(),
        }
    }
}

/// wrist（腕時計）の文字盤の種類と、種類ごとのデザインの定義表。ここに 1 行追加すると、選択画面の一覧にも表示される
/// （描画は `wrist` クレート。追加方法は `crates/wrist/AGENT.md`）。名前は設定ファイルの値と文言のキーに使用する。
macro_rules! wrist_designs {
    ($($face:ident = $face_key:literal { $($design:ident = $key:literal),+ $(,)? })+) => {
        /// wrist の文字盤の種類。
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum WristFace { $($face),+ }

        /// wrist の文字盤のデザイン（`wrist_design`）。どれも 1 つの種類（[`WristDesign::face`]）に属する。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum WristDesign { $($(#[serde(rename = $key)] $design),+),+ }

        impl WristFace {
            pub const ALL: &[WristFace] = &[$(Self::$face),+];

            /// 文言のキーに使う名前（`wrist.face.<名前>`）。
            pub const fn key(self) -> &'static str {
                match self { $(Self::$face => $face_key),+ }
            }

            /// この種類のデザイン（先頭がこの種類の既定）。
            pub const fn designs(self) -> &'static [WristDesign] {
                match self { $(Self::$face => &[$(WristDesign::$design),+]),+ }
            }
        }

        impl WristDesign {
            /// 設定ファイルの値で、文言のキーにも使う名前（`wrist.design.<名前>`）。
            pub const fn key(self) -> &'static str {
                match self { $($(Self::$design => $key),+),+ }
            }

            pub const fn face(self) -> WristFace {
                match self { $($(Self::$design => WristFace::$face),+),+ }
            }
        }
    };
}

wrist_designs! {
    Standard = "standard" { Dots = "dots", Ring = "ring", Gauge = "gauge" }
    Digital = "digital" { Lcd = "lcd", Flip = "flip", Terminal = "terminal" }
    Analog = "analog" { Minimal = "minimal", Dress = "dress", Railway = "railway" }
}

impl Default for WristDesign {
    /// 既定は先頭の種類の先頭のデザイン。
    fn default() -> Self {
        WristFace::ALL[0].designs()[0]
    }
}

/// 利き手（`right` / `left`）。主に操作する手。左右で動作が変わる機能が、それぞれこの値を参照して決定する。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DominantHand {
    #[default]
    Right,
    Left,
}

impl DominantHand {
    pub fn hand(self) -> vr::Hand {
        match self {
            Self::Right => vr::Hand::Right,
            Self::Left => vr::Hand::Left,
        }
    }

    /// 利き手でない方の手。
    pub fn off_hand(self) -> vr::Hand {
        match self {
            Self::Right => vr::Hand::Left,
            Self::Left => vr::Hand::Right,
        }
    }
}

/// 通知を視界の端に表示する位置（HUD。`top_left` / `top` / `top_right` / `left` / `right` / `bottom_left` / `bottom` /
/// `bottom_right`）。視界の上下左右の端と四隅の 8 か所。カーネルは値を保持するのみで、配置は通知画面が行う。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    #[default]
    Bottom,
    BottomRight,
}

impl NotificationAnchor {
    /// 選択画面に並べる順（左上から右下へ、中央を除いた 3×3 のグリッド）。
    pub const ALL: [Self; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    /// 視界の中での向き（左 -1・中 0・右 1、下 -1・中 0・上 1）。
    pub fn side(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (-1.0, 1.0),
            Self::Top => (0.0, 1.0),
            Self::TopRight => (1.0, 1.0),
            Self::Left => (-1.0, 0.0),
            Self::Right => (1.0, 0.0),
            Self::BottomLeft => (-1.0, -1.0),
            Self::Bottom => (0.0, -1.0),
            Self::BottomRight => (1.0, -1.0),
        }
    }
}

/// アップデートのチャンネル（安定版 `stable` / ナイトリー `nightly`）。安定版は正式リリースのみ、ナイトリーはプレリリース
/// （alpha・beta など。GitHub でプレリリースとしてマークされたもの）も通知する。カーネルは値を保持するのみで、解釈は `updater` が行う。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Nightly,
}

/// `config.toml` の内容。トップレベルにはテーブルだけを置き、値はすべていずれかのテーブルに入れる。テーブルの名前は、
/// その値を受け取って動作を変えるクレートの名前にする（カーネルが値を保持するだけの設定も、解釈するクレートのテーブルに入れる）。
/// テーブルごとに構造体を 1 つ定義し、このフィールドにする。値をトップレベルに直接置くと、どのクレートの設定かをキーから
/// 判別できなくなるためである。この決まりはテスト（`every_top_level_key_is_a_crate_table`）で検査する。
/// フィールドは `AGENT.md` のクレート一覧と同じ層の順に並べる。すべてのキーは省略可能で、未知のキーは無視する。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub logger: LoggerTable,
    pub vr: VrTable,
    pub input: InputTable,
    pub keyboard: KeyboardTable,
    pub desktop: DesktopTable,
    pub overlay: OverlayTable,
    pub kernel: KernelTable,
    pub ui: UiTable,
    pub design: DesignTable,
    pub i18n: I18nTable,
    pub wrist: WristTable,
    pub toast: ToastTable,
    pub dashboard: DashboardTable,
    pub settings: SettingsTable,
    pub setup: SetupTable,
    pub updater: UpdaterTable,
}

/// `[logger]`: ログの出力。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggerTable {
    /// ログレベル（`trace` / `debug` / `info` / `warn` / `error`）。環境変数 `NYAW_LOG` があればそちらを優先する。解釈は呼び出し側。
    pub level: Option<String>,
}

/// `[vr]`: VR ランタイムとエミュレーター。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VrTable {
    /// novr のエミュレーターの合成に GPU を使うか。省略で `true`。`false` にすると CPU で合成する（互換性のための切り替え）。
    pub emulator_gpu: bool,
}

impl Default for VrTable {
    fn default() -> Self {
        Self { emulator_gpu: true }
    }
}

/// `[input]`: コントローラと手の入力。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputTable {
    /// コントローラを持たない手（スケルタル入力）の判定しきい値とレーザーの揺れ抑制（`[input.hand]`）。書いたキーだけが変わり、
    /// ほかは既定値になる。画面での調整はここへテーブルごと書き戻される。
    pub hand: subsystems::input::HandTuning,
}

/// `[keyboard]`: 仮想キーボード。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyboardTable {
    /// 種類（`full` / `tenkeyless` / `compact`）。省略で `full`。画面での切り替えはここへ書き戻される。
    pub kind: subsystems::keyboard::KeyboardType,
    /// 大きさの倍率（[`KEYBOARD_SCALE_MIN`]〜[`KEYBOARD_SCALE_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub scale: Option<f32>,
    /// キーの間隔の倍率（[`KEY_GAP_MIN`]〜[`KEY_GAP_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub key_gap: Option<f32>,
    /// キーを押した音を鳴らすか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub key_sound: Option<bool>,
    /// キーを押した音の音量（0〜1。[`SCALE_STEP`] の刻み）。省略で 1。画面での切り替えはここへ書き戻される。
    pub key_sound_volume: Option<f32>,
}

/// `[desktop]`: ミラーリング（デスクトップの画面・ウィンドウの配信）。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopTable {
    /// 取得の上限 FPS（1〜240）。省略で `desktop::DEFAULT_FRAME_RATE`。画面での切り替えはここへ書き戻される。
    pub frame_rate: Option<u32>,
}

/// `[overlay]`: パネルの大きさ、レーザーとカーソル、空間の暗転。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayTable {
    /// UI スケール: パネルの大きさの倍率（[`UI_SCALE_MIN`]〜[`UI_SCALE_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub ui_scale: Option<f32>,
    /// レーザーの手ブレ抑え（0 = 抑えない 〜 1 未満。大きいほど強く、遅れる）。省略で既定値。
    pub pointer_smoothing: Option<f32>,
    /// 手ブレ抑えを緩める角度（度）。抑えたレーザーが手の向きからこれだけ遅れると抑えが半分になる。省略で既定値。
    pub pointer_release_deg: Option<f32>,
    /// レーザーの見える長さ（m）。手元からこの長さで消える（カーソルが近ければその手前で消える）。省略で既定値。
    pub laser_length: Option<f32>,
    /// カーソルの大きさの倍率（[`CURSOR_SCALE_MIN`]〜[`CURSOR_SCALE_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub cursor_scale: Option<f32>,
    /// ダッシュボードが出ている間、ゲームへの入力を止めるか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub block_input: Option<bool>,
    /// 空間全体の暗転幕の濃さ（黒の不透明度）。ダッシュボードの表示中に背景を暗くする度合いで、0 で暗くせず、1 で真っ黒になる。
    /// 刻みは [`SCALE_STEP`]。省略で `overlay::VEIL`。画面での切り替えはここへ書き戻される。
    pub veil_opacity: Option<f32>,
}

/// `[kernel]`: カーネル自身が解釈する設定。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KernelTable {
    /// 利き手（`right` / `left`）。省略で `right`。画面での切り替えはここへ書き戻される。
    pub dominant_hand: DominantHand,
    /// レーザーのエミュレート方式（`mouse` / `touch`）。省略でタッチ。設定画面には無く、書き戻されない。
    pub pointer_mode: Option<PointerMode>,
    /// 距離 LOD の品質（0 = Off、1 = Low、2 = Mid、3 = High）。省略で 2。画面での切り替えはここへ書き戻される。
    pub lod_quality: LodQuality,
}

/// `[ui]`: ツールチップと操作音。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiTable {
    /// 指している要素の説明（ツールチップ）を表示するか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub tooltips: Option<bool>,
    /// タップ時・ホバー時の効果音を再生するか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub tap_sound: Option<bool>,
    pub hover_sound: Option<bool>,
}

/// `[design]`: テーマ、パーソナルカラー、文字。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesignTable {
    /// 見た目のテーマ（`dark` / `light`）。省略で `dark`。画面での切り替えはここへ書き戻される。
    pub theme: ThemeMode,
    /// パーソナルカラー（`lightness` / `chroma` / `hue` の表）。省略で既定の色。画面での切り替えはここへ書き戻される。
    pub personal_color: Option<PersonalColor>,
    /// 自分で作ったパーソナルカラーの履歴（色の表の配列。新しい順で [`COLOR_HISTORY`] 個まで）。省略で空。作るたびにここへ書き戻される。
    pub color_history: Vec<PersonalColor>,
    /// 文字の大きさの倍率（[`TEXT_SCALE_MIN`]〜[`TEXT_SCALE_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub text_scale: Option<f32>,
    /// すべての文字を太字にするか。省略で `false`。画面での切り替えはここへ書き戻される。
    pub bold_text: bool,
}

/// `[i18n]`: 画面の言語。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct I18nTable {
    /// 画面の言語（`en` / `ja`）。省略で OS の表示言語（対応していなければ `en`）。画面での切り替えはここへ書き戻される。
    pub language: Option<Language>,
}

/// `[wrist]`: 腕時計パネル。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WristTable {
    /// 文字盤のデザイン（[`WristDesign::key`]。`dots` / `lcd` / `minimal` など）。省略で `dots`。画面での切り替えはここへ書き戻される。
    pub design: WristDesign,
    /// 掴んで置き直した姿勢（`position` / `orientation` の表。[`WristOffset`]）。省略で手首の既定の位置。
    /// 掴んで離すとここへ書き戻される。
    pub offset: Option<WristOffset>,
    /// 大きさの倍率（[`WRIST_SCALE_MIN`]〜[`WRIST_SCALE_MAX`]）。省略で等倍。掴んで大きさを変えるとここへ書き戻される。
    pub scale: Option<f32>,
}

/// `[toast]`: 通知のトースト表示。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToastTable {
    /// 通知を視界の端に出す位置（[`NotificationAnchor`]）。省略で `bottom`。画面での切り替えはここへ書き戻される。
    pub anchor: NotificationAnchor,
}

/// `[dashboard]`: ダッシュボードの置き場所。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DashboardTable {
    /// 距離（m。[`DASHBOARD_DISTANCE_MIN`]〜[`DASHBOARD_DISTANCE_MAX`]）。省略で [`DASHBOARD_DISTANCE`]。
    pub distance: Option<f32>,
    /// 高さ（m。[`DASHBOARD_HEIGHT_MIN`]〜[`DASHBOARD_HEIGHT_MAX`]）。目の高さの 0.1m 下を 0 とし、上を正とする。省略で 0。
    pub height: Option<f32>,
}

/// `[settings]`: 設定画面の表示。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsTable {
    /// ポインタの動きの調整のしかた（`simple` / `detailed`）。省略で `simple`。画面での切り替えはここへ書き戻される。
    pub pointer_tuning: PointerTuning,
}

/// `[setup]`: セットアップウィザード。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SetupTable {
    /// ウィザードを終えたか。省略で `false`（初回起動でウィザードを出す）。終えるとここへ書き戻される。
    pub done: bool,
}

/// `[updater]`: アップデートの確認。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdaterTable {
    /// アップデートのチャンネル（`stable` / `nightly`）。省略で `stable`。画面での切り替えはここへ書き戻される。
    pub channel: UpdateChannel,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Parse(toml::de::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Parse(e) => e.fmt(f),
        }
    }
}

/// 設定ファイルのパス。ディレクトリが決定できなければ `None`。
pub fn path() -> Option<PathBuf> {
    os::config_dir().map(|d| d.join("config.toml"))
}

pub fn parse(text: &str) -> Result<Config, Error> {
    toml::from_str(text).map_err(Error::Parse)
}

/// 設定ファイルを読み込む。ディレクトリが決定できない場合やファイルが存在しない場合は既定値を返す。
pub fn load() -> Result<Config, Error> {
    let Some(path) = path() else {
        return Ok(Config::default());
    };
    match fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(Error::Io(e)),
    }
}

/// 設定の値（列挙型は文字列）を、設定ファイルに書き込む TOML 値に変換する。
pub fn value(v: impl serde::Serialize) -> toml::Value {
    toml::Value::try_from(v).expect("settings serialize to TOML values")
}

/// `path` の設定ファイルのテーブル `table` の 1 つのキー `key` のみを書き換える（他のキーは保持する。ファイルやテーブルが
/// 存在しなければ作成する）。既存のファイルが読み込めない・TOML として不正な場合は、上書きせずにエラーを返す。
pub fn save_at(
    path: &Path,
    table: &str,
    key: &str,
    value: impl Into<toml::Value>,
) -> Result<(), Error> {
    let value = value.into();
    let mut doc = match fs::read_to_string(path) {
        Ok(text) => text.parse::<toml::Table>().map_err(Error::Parse)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(Error::Io(e)),
    };
    // テーブルの位置にテーブル以外の値があれば、起動時の読み込みが失敗して既定値で動いているので、テーブルで置き換える。
    let mut section = match doc.remove(table) {
        Some(toml::Value::Table(t)) => t,
        _ => toml::Table::new(),
    };
    section.insert(key.into(), value);
    doc.insert(table.into(), section.into());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(Error::Io)?;
    }
    // ponytail: 書き換えでコメントは消える。残したければ toml_edit。
    fs::write(path, doc.to_string()).map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// トップレベルはすべてテーブルで、名前はワークスペースのクレートの名前（[`Config`] の決まり）。JSON に変換して調べるのは、
    /// TOML への変換では `None` のフィールドが落ちて、トップレベルに直接置いた `Option` の値を見逃すため。
    #[test]
    fn every_top_level_key_is_a_crate_table() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let serde_json::Value::Object(top) = serde_json::to_value(Config::default()).unwrap()
        else {
            panic!("Config はテーブル");
        };
        assert!(!top.is_empty());
        for (name, value) in top {
            assert!(
                value.is_object(),
                "{name}: 値はトップレベルに置かず、クレートのテーブルの構造体に入れる"
            );
            let is_crate = crates.join(&name).join("Cargo.toml").is_file()
                || crates
                    .join("subsystems")
                    .join(&name)
                    .join("Cargo.toml")
                    .is_file();
            assert!(is_crate, "{name}: テーブルの名前はクレートの名前にする");
        }
    }

    /// `[input.hand]` は書いたキーだけ変わり、ほかは既定値。範囲の外はカーネルが範囲に収める。
    #[test]
    fn hand_table_overrides_only_given_keys() {
        let c = parse(
            "[input.hand]
pinch_mm = 32.0
smoothing = 0.2
",
        )
        .unwrap();
        assert_eq!(c.input.hand.pinch_mm, 32.0);
        assert_eq!(c.input.hand.smoothing, 0.2);
        let d = subsystems::input::HandTuning::default();
        assert_eq!(c.input.hand.grip_deg, d.grip_deg);
        assert_eq!(parse("").unwrap().input.hand, d);
    }

    /// 以前のトップレベルのキーは読まない（互換性は持たない）。未知のキーと同じく無視して既定値になる。
    #[test]
    fn old_top_level_keys_are_ignored() {
        let c = parse("setup_done = true\ntheme = \"light\"\nruntime = \"novr\"").unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn setup_done_defaults_to_false() {
        assert!(!parse("").unwrap().setup.done);
        assert!(parse("[setup]\ndone = true").unwrap().setup.done);
    }

    #[test]
    fn lod_quality_defaults_to_mid_and_validates_numbers() {
        assert_eq!(parse("").unwrap().kernel.lod_quality, LodQuality::Mid);
        for (n, quality) in [
            (0, LodQuality::Off),
            (1, LodQuality::Low),
            (2, LodQuality::Mid),
            (3, LodQuality::High),
        ] {
            let text = format!("kernel.lod_quality = {n}");
            assert_eq!(parse(&text).unwrap().kernel.lod_quality, quality);
        }
        for value in ["4", "-1", "2.5", "\"Mid\""] {
            assert!(parse(&format!("kernel.lod_quality = {value}")).is_err());
        }
    }

    #[test]
    fn parses_known_keys_and_ignores_unknown() {
        let c = parse(
            "[logger]
level = \"debug\"
[overlay]
pointer_smoothing = 0.5
future = 1
[future]
key = 1
",
        )
        .unwrap();
        assert_eq!(c.overlay.pointer_smoothing, Some(0.5));
        assert_eq!(c.logger.level.as_deref(), Some("debug"));
    }

    #[test]
    fn emulator_gpu_defaults_on_and_switches_off() {
        assert!(Config::default().vr.emulator_gpu);
        assert!(!parse("[vr]\nemulator_gpu = false").unwrap().vr.emulator_gpu);
    }

    #[test]
    fn empty_is_default_and_bad_toml_is_error() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert!(parse("[logger]\nlevel = ").is_err());
        // テーブルの位置に値を置くのは不正。
        assert!(parse("design = \"light\"").is_err());
    }

    #[test]
    fn theme_defaults_to_dark_and_parses() {
        assert_eq!(parse("").unwrap().design.theme, ThemeMode::Dark);
        let light = parse("design.theme = \"light\"").unwrap();
        assert_eq!(light.design.theme, ThemeMode::Light);
        assert!(parse("design.theme = \"blue\"").is_err());
        assert_eq!(parse("").unwrap().i18n.language, None);
        let ja = parse("i18n.language = \"ja\"").unwrap().i18n.language;
        assert_eq!(ja, Some(Language::Ja));
        assert_eq!(Language::from_code("ja"), Some(Language::Ja));
        assert_eq!(Language::from_code("fr"), None);
        assert!(parse("i18n.language = \"fr\"").is_err());
        assert_eq!(ThemeMode::Dark.toggled(), ThemeMode::Light);
        let right = parse("").unwrap().kernel.dominant_hand;
        assert_eq!(right, DominantHand::Right);
        let left = parse("kernel.dominant_hand = \"left\"").unwrap();
        let left = left.kernel.dominant_hand;
        assert_eq!(
            (left.hand(), left.off_hand()),
            (vr::Hand::Left, vr::Hand::Right)
        );
        assert!(parse("kernel.dominant_hand = \"both\"").is_err());
        assert_eq!(parse("").unwrap().toast.anchor, NotificationAnchor::Bottom);
        for anchor in NotificationAnchor::ALL {
            let text = format!("toast.anchor = {}", value(anchor));
            assert_eq!(parse(&text).unwrap().toast.anchor, anchor);
        }
        let corner = parse("toast.anchor = \"bottom_right\"").unwrap();
        assert_eq!(corner.toast.anchor.side(), (1.0, -1.0));
        assert!(parse("toast.anchor = \"center\"").is_err());
        assert!(parse("wrist.design = \"sundial\"").is_err());
        let stable = parse("").unwrap().updater.channel;
        assert_eq!(stable, UpdateChannel::Stable);
        let nightly = parse("updater.channel = \"nightly\"").unwrap();
        assert_eq!(nightly.updater.channel, UpdateChannel::Nightly);
        assert!(parse("updater.channel = \"beta\"").is_err());
    }

    #[test]
    fn personal_color_parses_clamps_and_round_trips() {
        assert_eq!(parse("").unwrap().design.personal_color, None);
        let c = parse("design.personal_color = { lightness = 0.66, chroma = 0.21, hue = 350.0 }")
            .unwrap();
        let pink = PersonalColor {
            lightness: 0.66,
            chroma: 0.21,
            hue: 350.0,
        };
        assert_eq!(c.design.personal_color, Some(pink));
        assert!(
            parse("design.personal_color = { hue = 350.0 }").is_err(),
            "3 つとも要る"
        );
        let wild = PersonalColor {
            lightness: 2.0,
            chroma: 7.0,
            hue: -30.0,
        };
        let tame = wild.clamped();
        assert_eq!((tame.lightness, tame.chroma, tame.hue), (1.0, 0.4, 330.0));
        let nan =
            parse("design.personal_color = { lightness = nan, chroma = nan, hue = nan }").unwrap();
        let sane = nan.design.personal_color.unwrap().clamped();
        assert_eq!((sane.lightness, sane.chroma, sane.hue), (0.0, 0.0, 0.0));
        // 表ごと書き戻せ、読み直すと同じ値になる（f32 を TOML に通しても変わらない）。
        let dir = std::env::temp_dir().join(format!("nyaw-color-{}", std::process::id()));
        let path = dir.join("config.toml");
        let blue = PersonalColor {
            lightness: 0.6493,
            chroma: 0.193,
            hue: 251.78,
        };
        save_at(&path, "design", "personal_color", value(blue)).unwrap();
        // 履歴は色の表の並び。
        assert_eq!(parse("").unwrap().design.color_history, []);
        save_at(&path, "design", "color_history", value(vec![pink, blue])).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.design.personal_color, Some(blue));
        assert_eq!(back.design.color_history, [pink, blue]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 文字盤のデザインは表の名前で読み書きでき、どれも自分の種類の一覧に 1 度だけ載っている。
    #[test]
    fn wrist_designs_round_trip_and_belong_to_one_face() {
        assert_eq!(parse("").unwrap().wrist.design, WristDesign::Dots);
        let mut seen = Vec::new();
        for &face in WristFace::ALL {
            assert!(face.designs().len() >= 3, "{face:?}: 種類ごとに 3 つ以上");
            for &design in face.designs() {
                assert_eq!(design.face(), face);
                let text = format!("wrist.design = \"{}\"", design.key());
                assert_eq!(parse(&text).unwrap().wrist.design, design);
                assert_eq!(value(design).as_str(), Some(design.key()));
                assert!(!seen.contains(&design.key()), "{} が重複", design.key());
                seen.push(design.key());
            }
        }
    }

    /// wrist の姿勢は `wrist.offset` で読み書きでき、向きは長さ 1 に直す。壊れた値は姿勢にしない。
    #[test]
    fn wrist_offset_round_trips_and_rejects_broken_values() {
        assert_eq!(parse("").unwrap().wrist.offset, None);
        let pose = vr::Pose::new(
            vr::Vec3::new(-0.1, 0.02, 0.15),
            vr::Quat::from_rotation_y(0.5),
        );
        let text = format!("wrist.offset = {}", value(WristOffset::from(pose)));
        let back = parse(&text).unwrap().wrist.offset.unwrap().pose().unwrap();
        assert_eq!(back.position, pose.position);
        assert!(back.orientation.abs_diff_eq(pose.orientation, 1e-6));
        let read = |text| parse(text).unwrap().wrist.offset.unwrap().pose();
        let scaled = read("wrist.offset = { position = [0, 0, 0], orientation = [0, 0, 0, 2] }");
        assert_eq!(scaled, Some(vr::Pose::IDENTITY));
        assert_eq!(
            read("wrist.offset = { position = [0, 0, 0], orientation = [0, 0, 0, 0] }"),
            None
        );
        assert_eq!(
            read("wrist.offset = { position = [nan, 0, 0], orientation = [0, 0, 0, 1] }"),
            None
        );
        assert!(parse("wrist.offset = { position = [0, 0], orientation = [0, 0, 0, 1] }").is_err());
    }

    #[test]
    fn mirror_frame_rate_and_keyboard_parse() {
        use subsystems::keyboard::KeyboardType;
        let c = parse("").unwrap();
        assert_eq!(
            (c.desktop.frame_rate, c.keyboard.kind),
            (None, KeyboardType::Full)
        );
        let c = parse("desktop.frame_rate = 30\nkeyboard.kind = \"tenkeyless\"").unwrap();
        assert_eq!(
            (c.desktop.frame_rate, c.keyboard.kind),
            (Some(30), KeyboardType::Tenkeyless)
        );
        assert!(parse("keyboard.kind = \"split\"").is_err());
        assert_eq!(parse("").unwrap().overlay.block_input, None);
        let off = parse("overlay.block_input = false").unwrap();
        assert_eq!(off.overlay.block_input, Some(false));
    }

    /// アクセシビリティの 3 つは独立したキーで、どれも省略でき、書き戻しても互いを消さない。
    #[test]
    fn accessibility_keys_parse_clamp_and_round_trip() {
        let c = parse("").unwrap();
        assert_eq!(
            (c.design.text_scale, c.overlay.ui_scale, c.design.bold_text),
            (None, None, false)
        );
        let c = parse("design.text_scale = 1.3\noverlay.ui_scale = 0.8\ndesign.bold_text = true")
            .unwrap();
        assert_eq!(
            (c.design.text_scale, c.overlay.ui_scale, c.design.bold_text),
            (Some(1.3), Some(0.8), true)
        );
        assert!(parse("design.bold_text = \"yes\"").is_err());
        // 範囲の外は端へ、刻みの間は近い刻みへ、数でない値は等倍へ。
        let text = |s| clamp_scale(s, TEXT_SCALE_MIN, TEXT_SCALE_MAX);
        assert_eq!(
            [text(9.0), text(0.0), text(-3.0)],
            [TEXT_SCALE_MAX, TEXT_SCALE_MIN, TEXT_SCALE_MIN]
        );
        assert_eq!([text(f32::NAN), text(f32::INFINITY)], [1.0, 1.0]);
        assert!((text(1.234) - 1.2).abs() < 1e-6);
        assert!((clamp_scale(0.71, UI_SCALE_MIN, UI_SCALE_MAX) - UI_SCALE_MIN).abs() < 1e-6);
        // 刻みは両方の範囲の端にちょうど乗る。
        for (min, max) in [
            (TEXT_SCALE_MIN, TEXT_SCALE_MAX),
            (UI_SCALE_MIN, UI_SCALE_MAX),
        ] {
            assert!((clamp_scale(min, min, max) - min).abs() < 1e-6);
            assert!((clamp_scale(max, min, max) - max).abs() < 1e-6);
            assert!(min < 1.0 && 1.0 < max, "等倍は範囲の中");
        }
        let dir = std::env::temp_dir().join(format!("nyaw-a11y-{}", std::process::id()));
        let path = dir.join("config.toml");
        save_at(&path, "design", "text_scale", value(1.5f32)).unwrap();
        save_at(&path, "overlay", "ui_scale", value(0.7f32)).unwrap();
        save_at(&path, "design", "bold_text", value(true)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (
                back.design.text_scale,
                back.overlay.ui_scale,
                back.design.bold_text
            ),
            (Some(1.5), Some(0.7), true)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// キーの音の 2 つは独立したキーで、どちらも省略でき、書き戻しても互いを消さない。
    #[test]
    fn key_sound_keys_parse_and_round_trip() {
        let c = parse("").unwrap();
        assert_eq!(
            (c.keyboard.key_sound, c.keyboard.key_sound_volume),
            (None, None)
        );
        let c = parse("[keyboard]\nkey_sound = false\nkey_sound_volume = 0.4").unwrap();
        assert_eq!(
            (c.keyboard.key_sound, c.keyboard.key_sound_volume),
            (Some(false), Some(0.4))
        );
        assert!(parse("keyboard.key_sound = \"off\"").is_err());
        // 音量は 0〜1 の刻みに収まる（数でない値は最大）。
        let volume = |v| clamp_scale(v, 0.0, 1.0);
        assert_eq!(
            [volume(-1.0), volume(7.0), volume(f32::NAN)],
            [0.0, 1.0, 1.0]
        );
        assert!((volume(0.44) - 0.4).abs() < 1e-6);
        let dir = std::env::temp_dir().join(format!("nyaw-key-sound-{}", std::process::id()));
        let path = dir.join("config.toml");
        save_at(&path, "keyboard", "key_sound", value(false)).unwrap();
        save_at(&path, "keyboard", "key_sound_volume", value(0.3f32)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (back.keyboard.key_sound, back.keyboard.key_sound_volume),
            (Some(false), Some(0.3))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// ダッシュボードの距離・高さと暗転幕の濃さはどれも省略でき、書き戻しても互いを消さない。`snap` は範囲と刻みに収める。
    #[test]
    fn dashboard_keys_parse_round_trip_and_snap() {
        let c = parse("").unwrap();
        assert_eq!(
            (
                c.dashboard.distance,
                c.dashboard.height,
                c.overlay.veil_opacity
            ),
            (None, None, None)
        );
        let c = parse("[dashboard]\ndistance = 1.5\nheight = -0.2").unwrap();
        assert_eq!(
            (c.dashboard.distance, c.dashboard.height),
            (Some(1.5), Some(-0.2))
        );
        let (min, max) = (DASHBOARD_HEIGHT_MIN, DASHBOARD_HEIGHT_MAX);
        let height = |v| snap(v, min, max, DASHBOARD_STEP, 0.0);
        assert_eq!(
            [height(-9.0), height(9.0), height(f32::NAN)],
            [min, max, 0.0]
        );
        assert!((height(0.12) - 0.1).abs() < 1e-6);
        // 刻みの上の値は誤差なくそのまま（1.3 が 1.3000001 にならない）。
        assert_eq!(snap(1.3, 0.6, 2.0, DASHBOARD_STEP, 1.0), 1.3);
        // 0 の近くの負の値は -0 にせず 0 にする（表示が「-0.00」にならない）。
        assert!(height(-0.01).is_sign_positive());
        let dir = std::env::temp_dir().join(format!("nyaw-dashboard-{}", std::process::id()));
        let path = dir.join("config.toml");
        save_at(&path, "dashboard", "distance", value(0.8f32)).unwrap();
        save_at(&path, "dashboard", "height", value(0.25f32)).unwrap();
        save_at(&path, "overlay", "veil_opacity", value(0.75f32)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (
                back.dashboard.distance,
                back.dashboard.height,
                back.overlay.veil_opacity
            ),
            (Some(0.8), Some(0.25), Some(0.75))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// ポインタの画面のキー（調整のしかた・カーソルの大きさ・ツールチップ・タップ音・ホバー音）はどれも省略でき、書き戻しても互いを消さない。
    #[test]
    fn pointer_keys_parse_and_round_trip() {
        let c = parse("").unwrap();
        assert_eq!(c.settings.pointer_tuning, PointerTuning::Simple);
        assert_eq!(
            (
                c.overlay.cursor_scale,
                c.ui.tooltips,
                c.ui.tap_sound,
                c.ui.hover_sound
            ),
            (None, None, None, None)
        );
        let c = parse(
            "settings.pointer_tuning = \"detailed\"\noverlay.cursor_scale = 1.5\n\
             [ui]\ntooltips = false\ntap_sound = false\nhover_sound = true",
        )
        .unwrap();
        assert_eq!(c.settings.pointer_tuning, PointerTuning::Detailed);
        assert_eq!(
            (
                c.overlay.cursor_scale,
                c.ui.tooltips,
                c.ui.tap_sound,
                c.ui.hover_sound
            ),
            (Some(1.5), Some(false), Some(false), Some(true))
        );
        assert!(parse("settings.pointer_tuning = \"expert\"").is_err());
        // 刻みは範囲の端にちょうど乗り、等倍は範囲の中。
        let cursor = |s| clamp_scale(s, CURSOR_SCALE_MIN, CURSOR_SCALE_MAX);
        assert_eq!(
            [cursor(0.0), cursor(9.0)],
            [CURSOR_SCALE_MIN, CURSOR_SCALE_MAX]
        );
        const { assert!(CURSOR_SCALE_MIN < 1.0 && 1.0 < CURSOR_SCALE_MAX) };
        let dir = std::env::temp_dir().join(format!("nyaw-pointer-{}", std::process::id()));
        let path = dir.join("config.toml");
        save_at(
            &path,
            "settings",
            "pointer_tuning",
            value(PointerTuning::Detailed),
        )
        .unwrap();
        save_at(&path, "overlay", "cursor_scale", value(0.5f32)).unwrap();
        save_at(&path, "ui", "tooltips", value(false)).unwrap();
        save_at(&path, "ui", "tap_sound", value(false)).unwrap();
        save_at(&path, "ui", "hover_sound", value(false)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.settings.pointer_tuning, PointerTuning::Detailed);
        assert_eq!(
            (
                back.overlay.cursor_scale,
                back.ui.tooltips,
                back.ui.tap_sound,
                back.ui.hover_sound
            ),
            (Some(0.5), Some(false), Some(false), Some(false))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_keeps_other_keys_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("nyaw-config-{}", std::process::id()));
        let path = dir.join("nested").join("config.toml");
        // ファイルもディレクトリも無くても作る。
        save_at(&path, "design", "theme", value(ThemeMode::Light)).unwrap();
        assert_eq!(
            parse(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .design
                .theme,
            ThemeMode::Light
        );
        fs::write(
            &path,
            "[logger]
level = \"debug\"
[vr]
emulator_gpu = false
[design]
bold_text = true
",
        )
        .unwrap();
        // 同じテーブルの他のキーも、他のテーブルも残る。
        save_at(&path, "design", "theme", value(ThemeMode::Light)).unwrap();
        let c = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (
                c.design.theme,
                c.design.bold_text,
                c.logger.level.as_deref(),
                c.vr.emulator_gpu
            ),
            (ThemeMode::Light, true, Some("debug"), false)
        );
        // 手ブレ抑えも同じく書き戻せ、テーマは残る。
        save_at(&path, "overlay", "pointer_smoothing", 0.5).unwrap();
        let c = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (c.overlay.pointer_smoothing, c.design.theme),
            (Some(0.5), ThemeMode::Light)
        );
        // 言語も同じく書き戻せ、テーマは残る。
        save_at(&path, "i18n", "language", value(Language::Ja)).unwrap();
        let c = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (c.i18n.language, c.design.theme),
            (Some(Language::Ja), ThemeMode::Light)
        );
        // テーブルの位置にあるテーブル以外の値は、テーブルで置き換える。
        fs::write(&path, "setup = 1").unwrap();
        save_at(&path, "setup", "done", value(true)).unwrap();
        assert!(
            parse(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .setup
                .done
        );
        // 壊れたファイルは上書きしない。
        fs::write(&path, "[logger]\nlevel = ").unwrap();
        assert!(save_at(&path, "design", "theme", value(ThemeMode::Dark)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[logger]\nlevel = ");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_is_config_toml_under_config_dir() {
        assert_eq!(path(), os::config_dir().map(|d| d.join("config.toml")));
    }
}
