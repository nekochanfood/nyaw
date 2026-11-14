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

/// トリガーを押したまま動かしたときの意味。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerMode {
    /// マウスのエミュレート: 押したまま動かすとドラッグ。
    Mouse,
    /// タッチのエミュレート（既定）: 押したまま動かすとスワイプ（スクロール。離すと慣性で滑る）、動かさずに離すとタップ（クリック）、
    /// 動かさずに長押しするとマウスの押下（そのまま動かせばドラッグ）。
    #[default]
    Touch,
}

/// 設定画面のポインタの動きの調整のしかた。シンプルは 1 本のスライダー（手ブレ抑えの強さと緩める角度をまとめて動かす）、
/// 詳細はそれぞれのスライダー。カーネルは値を持つだけで、従うのは設定画面。
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

/// パーソナルカラー: ボタンなどの塗りに使う、ユーザーの色（OKLCH）。`design::Accent` に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PersonalColor {
    /// 明るさ（0〜1）。
    pub lightness: f32,
    /// 鮮やかさ（0〜0.4。画面に出せる範囲を越える分は、描くときに落とす）。
    pub chroma: f32,
    /// 色相（度。0〜360）。
    pub hue: f32,
}

impl PersonalColor {
    /// 範囲に収める（色相は 0〜360 に巻く。数でない値は 0）。
    pub fn clamped(self) -> Self {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        Self {
            lightness: finite(self.lightness).clamp(0.0, 1.0),
            chroma: finite(self.chroma).clamp(0.0, 0.4),
            hue: finite(self.hue).rem_euclid(360.0),
        }
    }
}

/// 文字の大きさの倍率（[`Config::text_scale`]）と UI スケール（[`Config::ui_scale`]）の範囲と刻み。画面の中の組み方は
/// 変えないので、文字は入れ物に収まる倍率まで。UI スケールはパネルごと大きくするので、視野に収まる倍率まで。
pub const TEXT_SCALE_MIN: f32 = 0.8;
pub const TEXT_SCALE_MAX: f32 = 1.5;
pub const UI_SCALE_MIN: f32 = 0.7;
pub const UI_SCALE_MAX: f32 = 1.5;
pub const SCALE_STEP: f32 = 0.1;
/// カーソルの大きさの倍率（[`Config::cursor_scale`]）の範囲。刻みは [`SCALE_STEP`]。
pub const CURSOR_SCALE_MIN: f32 = 0.5;
pub const CURSOR_SCALE_MAX: f32 = 2.0;

/// 倍率を範囲 `min..=max` に収め、[`SCALE_STEP`] の刻みに丸める（数でない値は等倍）。
pub fn clamp_scale(scale: f32, min: f32, max: f32) -> f32 {
    snap(scale, min, max, SCALE_STEP, 1.0)
}

/// 値を範囲 `min..=max` に収め、`step` の刻みに丸める（数でない値は `fallback`。-0 は 0 にする）。
pub fn snap(value: f32, min: f32, max: f32, step: f32, fallback: f32) -> f32 {
    let value = if value.is_finite() { value } else { fallback };
    ((value.clamp(min, max) / step).round() * step).clamp(min, max) + 0.0
}

/// ダッシュボードの置き場所（[`Config::dashboard_distance`] / [`Config::dashboard_height`]）の既定・範囲と刻み（m）。
/// 距離ははめた画面の中心まで。高さは頭の高さからの相対（上が +）で、既定の 0 は画面の中心が目の高さ。
pub const DASHBOARD_DISTANCE: f32 = 1.1;
pub const DASHBOARD_DISTANCE_MIN: f32 = 0.6;
pub const DASHBOARD_DISTANCE_MAX: f32 = 2.0;
pub const DASHBOARD_HEIGHT_MIN: f32 = -0.5;
pub const DASHBOARD_HEIGHT_MAX: f32 = 0.5;
pub const DASHBOARD_STEP: f32 = 0.05;

/// 自分で作ったパーソナルカラーを覚えておく数（[`Config::color_history`]）。
pub const COLOR_HISTORY: usize = 8;

/// wrist（腕時計）の文字盤の種類と、種類ごとのデザインの表。ここに 1 行足すと、選ぶ画面の一覧にも並ぶ
/// （描くのは `wrist` クレート。足し方は `crates/wrist/AGENT.md`）。名前は設定ファイルの値と文言のキーになる。
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

/// 利き手（`right` / `left`）。主に操作する手。左右で振る舞いが変わる機能が、それぞれこれを見て決める。
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

/// アップデートのチャンネル（`stable` / `nightly`）。stable は正式なリリースだけ、nightly はプレリリース
/// （alpha・beta など。GitHub でプレリリースの印が付いたもの）も勧める。カーネルは値を持つだけで、従うのは `updater`。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Nightly,
}

/// `config.toml` の中身。どのキーも省略でき、知らないキーは無視する。
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 距離 LOD の品質（0 = Off、1 = Low、2 = Mid、3 = High）。省略で 2。
    pub lod_quality: LodQuality,
    /// ログレベル（`trace` / `debug` / `info` / `warn` / `error`）。解釈は呼び出し側。
    pub log: Option<String>,
    /// 使うランタイム（`openvr` / `openxr` / `novr`）。解釈は呼び出し側。
    /// 非推奨: ランタイムはコマンドラインで指定する（main はデバッグビルドでだけ読み、リリースビルドでは無視する）。
    pub runtime: Option<String>,
    /// レーザーの手ブレ抑え（0 = 抑えない 〜 1 未満。大きいほど強く、遅れる）。省略で既定値。
    pub pointer_smoothing: Option<f32>,
    /// 手ブレ抑えを緩める角度（度）。抑えたレーザーが手の向きからこれだけ遅れると抑えが半分になる。省略で既定値。
    pub pointer_release_deg: Option<f32>,
    /// レーザーの見える長さ（m）。手元からこの長さで消える（カーソルが近ければその手前で消える）。省略で既定値。
    pub laser_length: Option<f32>,
    /// コントローラを持たない手（スケルタル入力）の判定のしきい値とレーザーの揺れの抑え（`[hand]` の表）。
    /// 画面での調整はここへ表ごと書き戻される。
    pub hand: subsystems::input::HandTuning,
    /// レーザーのエミュレート方式（`mouse` / `touch`）。省略でタッチ。
    pub pointer_mode: Option<PointerMode>,
    /// 設定画面のポインタの動きの調整のしかた（`simple` / `detailed`）。省略で `simple`。画面での切り替えはここへ書き戻される。
    pub pointer_tuning: PointerTuning,
    /// カーソルの大きさの倍率（[`CURSOR_SCALE_MIN`]〜[`CURSOR_SCALE_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub cursor_scale: Option<f32>,
    /// 指している要素の説明（ツールチップ）を出すか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub tooltips: Option<bool>,
    /// 押したとき（タップ）・指したとき（ホバー）の音を鳴らすか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub tap_sound: Option<bool>,
    pub hover_sound: Option<bool>,
    /// 機能ごとに GPU を使うか（`[gpu]` の表。互換性のため CPU に落とせる）。
    pub gpu: Gpu,
    /// 見た目のテーマ（`dark` / `light`）。省略で `dark`。画面での切り替えはここへ書き戻される。
    pub theme: ThemeMode,
    /// パーソナルカラー（`[personal_color]` の表: `lightness` / `chroma` / `hue`）。省略で既定の色。画面での切り替えはここへ書き戻される。
    pub personal_color: Option<PersonalColor>,
    /// 自分で作ったパーソナルカラーの履歴（`[[color_history]]`。新しい順で [`COLOR_HISTORY`] 個まで）。省略で空。作るたびにここへ書き戻される。
    pub color_history: Vec<PersonalColor>,
    /// 画面の言語（`en` / `ja`）。省略で OS の表示言語（対応していなければ `en`）。画面での切り替えはここへ書き戻される。
    pub language: Option<Language>,
    /// 利き手（`right` / `left`）。省略で `right`。画面での切り替えはここへ書き戻される。
    pub dominant_hand: DominantHand,
    /// wrist の文字盤のデザイン（[`WristDesign::key`]。`dots` / `lcd` / `minimal` など）。省略で `dots`。画面での切り替えはここへ書き戻される。
    pub wrist_design: WristDesign,
    /// ミラーリング（デスクトップの配信）の取得の上限 FPS（1〜240）。省略で `desktop::DEFAULT_FRAME_RATE`。
    pub mirror_frame_rate: Option<u32>,
    /// 仮想キーボードの種類（`full` / `tenkeyless` / `compact`）。省略で `full`。
    pub keyboard: subsystems::keyboard::KeyboardType,
    /// ダッシュボードが出ている間、ゲームへの入力を止めるか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub block_input: Option<bool>,
    /// セットアップウィザードを終えたか。省略で `false`（初回起動でウィザードを出す）。終えるとここへ書き戻される。
    pub setup_done: bool,
    /// 文字の大きさの倍率（[`TEXT_SCALE_MIN`]〜[`TEXT_SCALE_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub text_scale: Option<f32>,
    /// UI スケール: パネルの大きさの倍率（[`UI_SCALE_MIN`]〜[`UI_SCALE_MAX`]）。省略で等倍。画面での切り替えはここへ書き戻される。
    pub ui_scale: Option<f32>,
    /// すべての文字を太字にするか。省略で `false`。画面での切り替えはここへ書き戻される。
    pub bold_text: bool,
    /// 仮想キーボードのキーを押した音を鳴らすか。省略で `true`。画面での切り替えはここへ書き戻される。
    pub key_sound: Option<bool>,
    /// キーを押した音の音量（0〜1。[`SCALE_STEP`] の刻み）。省略で 1。画面での切り替えはここへ書き戻される。
    pub key_sound_volume: Option<f32>,
    /// アップデートのチャンネル（`stable` / `nightly`）。省略で `stable`。画面での切り替えはここへ書き戻される。
    pub update_channel: UpdateChannel,
    /// ダッシュボードの距離（m。[`DASHBOARD_DISTANCE_MIN`]〜[`DASHBOARD_DISTANCE_MAX`]）。省略で [`DASHBOARD_DISTANCE`]。
    pub dashboard_distance: Option<f32>,
    /// ダッシュボードの高さ（m。頭の高さからの相対で上が +。[`DASHBOARD_HEIGHT_MIN`]〜[`DASHBOARD_HEIGHT_MAX`]）。省略で 0。
    pub dashboard_height: Option<f32>,
}

/// 機能ごとに GPU を使うか。どれも既定は GPU（`true`）で、`false` で CPU の実装に切り替える。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Gpu {
    /// novr のエミュレーターの合成（`false` で CPU で合成する）。
    pub emulator: bool,
}

impl Default for Gpu {
    fn default() -> Self {
        Self { emulator: true }
    }
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

/// 設定ファイルの場所。置き場が決まらなければ `None`。
pub fn path() -> Option<PathBuf> {
    os::config_dir().map(|d| d.join("config.toml"))
}

pub fn parse(text: &str) -> Result<Config, Error> {
    toml::from_str(text).map_err(Error::Parse)
}

/// 設定ファイルを読む。置き場が決まらない・ファイルが無いときは既定値。
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

/// 設定ファイルが読めなかった（どこかの値が壊れている）ときに、`runtime` だけ拾い出す。
/// 既定値で起動すると novr の指定が消え、VR ランタイム（SteamVR）に繋ぎに行ってしまうので。
pub fn salvage_runtime() -> Option<String> {
    runtime_from(&fs::read_to_string(path()?).ok()?)
}

fn runtime_from(text: &str) -> Option<String> {
    let table = text.parse::<toml::Table>().ok()?;
    table.get("runtime")?.as_str().map(str::to_owned)
}

/// 設定の値（列挙は文字列）を、設定ファイルに書く TOML の値にする。
pub fn value(v: impl serde::Serialize) -> toml::Value {
    toml::Value::try_from(v).expect("settings serialize to TOML values")
}

/// `path` の設定ファイルの 1 つのキーだけを書き換える（他のキーは残す。ファイルが無ければ作る）。
/// 既存のファイルが読めない・TOML として壊れているときは、上書きせずにエラーを返す。
pub fn save_at(path: &Path, key: &str, value: impl Into<toml::Value>) -> Result<(), Error> {
    let value = value.into();
    let mut table = match fs::read_to_string(path) {
        Ok(text) => text.parse::<toml::Table>().map_err(Error::Parse)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(Error::Io(e)),
    };
    table.insert(key.into(), value);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(Error::Io)?;
    }
    // ponytail: 書き換えでコメントは消える。残したければ toml_edit。
    fs::write(path, table.to_string()).map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[hand]` は書いたキーだけ変わり、ほかは既定値。範囲の外はカーネルが範囲に収める。
    #[test]
    fn hand_table_overrides_only_given_keys() {
        let c = parse(
            "[hand]
pinch_mm = 32.0
smoothing = 0.2
",
        )
        .unwrap();
        assert_eq!(c.hand.pinch_mm, 32.0);
        assert_eq!(c.hand.smoothing, 0.2);
        let d = subsystems::input::HandTuning::default();
        assert_eq!(c.hand.grip_deg, d.grip_deg);
        assert_eq!(parse("").unwrap().hand, d);
    }

    #[test]
    fn runtime_survives_a_broken_value() {
        let text = "runtime = \"novr\"
lod_quality = 9
";
        assert!(parse(text).is_err());
        assert_eq!(runtime_from(text).as_deref(), Some("novr"));
    }

    #[test]
    fn setup_done_defaults_to_false() {
        assert!(!parse("").unwrap().setup_done);
        assert!(parse("setup_done = true").unwrap().setup_done);
    }

    #[test]
    fn lod_quality_defaults_to_mid_and_validates_numbers() {
        assert_eq!(parse("").unwrap().lod_quality, LodQuality::Mid);
        for (n, quality) in [
            (0, LodQuality::Off),
            (1, LodQuality::Low),
            (2, LodQuality::Mid),
            (3, LodQuality::High),
        ] {
            assert_eq!(
                parse(&format!("lod_quality = {n}")).unwrap().lod_quality,
                quality
            );
        }
        for value in ["4", "-1", "2.5", "\"Mid\""] {
            assert!(parse(&format!("lod_quality = {value}")).is_err());
        }
    }

    #[test]
    fn parses_known_keys_and_ignores_unknown() {
        let c = parse("log = \"debug\"\nruntime = \"novr\"\npointer_smoothing = 0.5\nfuture = 1\n")
            .unwrap();
        assert_eq!(c.pointer_smoothing, Some(0.5));
        assert_eq!(c.log.as_deref(), Some("debug"));
        assert_eq!(c.runtime.as_deref(), Some("novr"));
    }

    #[test]
    fn gpu_defaults_on_and_switches_per_feature() {
        assert_eq!(Config::default().gpu, Gpu::default());
        assert!(Gpu::default().emulator);
        let c = parse(
            "[gpu]
emulator = false
",
        )
        .unwrap();
        assert!(!c.gpu.emulator);
    }

    #[test]
    fn empty_is_default_and_bad_toml_is_error() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert!(parse("log = ").is_err());
    }

    #[test]
    fn theme_defaults_to_dark_and_parses() {
        assert_eq!(parse("").unwrap().theme, ThemeMode::Dark);
        assert_eq!(parse("theme = \"light\"").unwrap().theme, ThemeMode::Light);
        assert!(parse("theme = \"blue\"").is_err());
        assert_eq!(parse("").unwrap().language, None);
        let ja = parse("language = \"ja\"").unwrap().language;
        assert_eq!(ja, Some(Language::Ja));
        assert_eq!(Language::from_code("ja"), Some(Language::Ja));
        assert_eq!(Language::from_code("fr"), None);
        assert!(parse("language = \"fr\"").is_err());
        assert_eq!(ThemeMode::Dark.toggled(), ThemeMode::Light);
        assert_eq!(parse("").unwrap().dominant_hand, DominantHand::Right);
        let left = parse("dominant_hand = \"left\"").unwrap().dominant_hand;
        assert_eq!(
            (left.hand(), left.off_hand()),
            (vr::Hand::Left, vr::Hand::Right)
        );
        assert!(parse("dominant_hand = \"both\"").is_err());
        assert!(parse("wrist_design = \"sundial\"").is_err());
        assert_eq!(parse("").unwrap().update_channel, UpdateChannel::Stable);
        let nightly = parse("update_channel = \"nightly\"").unwrap();
        assert_eq!(nightly.update_channel, UpdateChannel::Nightly);
        assert!(parse("update_channel = \"beta\"").is_err());
    }

    #[test]
    fn personal_color_parses_clamps_and_round_trips() {
        assert_eq!(parse("").unwrap().personal_color, None);
        let c = parse("personal_color = { lightness = 0.66, chroma = 0.21, hue = 350.0 }").unwrap();
        let pink = PersonalColor {
            lightness: 0.66,
            chroma: 0.21,
            hue: 350.0,
        };
        assert_eq!(c.personal_color, Some(pink));
        assert!(
            parse("personal_color = { hue = 350.0 }").is_err(),
            "3 つとも要る"
        );
        let wild = PersonalColor {
            lightness: 2.0,
            chroma: 7.0,
            hue: -30.0,
        };
        let tame = wild.clamped();
        assert_eq!((tame.lightness, tame.chroma, tame.hue), (1.0, 0.4, 330.0));
        let nan = parse("personal_color = { lightness = nan, chroma = nan, hue = nan }").unwrap();
        let sane = nan.personal_color.unwrap().clamped();
        assert_eq!((sane.lightness, sane.chroma, sane.hue), (0.0, 0.0, 0.0));
        // 表ごと書き戻せ、読み直すと同じ値になる（f32 を TOML に通しても変わらない）。
        let dir = std::env::temp_dir().join(format!("nyaw-color-{}", std::process::id()));
        let path = dir.join("config.toml");
        let blue = PersonalColor {
            lightness: 0.6493,
            chroma: 0.193,
            hue: 251.78,
        };
        save_at(&path, "personal_color", value(blue)).unwrap();
        // 履歴は色の表の並び。
        assert_eq!(parse("").unwrap().color_history, []);
        save_at(&path, "color_history", value(vec![pink, blue])).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.personal_color, Some(blue));
        assert_eq!(back.color_history, [pink, blue]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 文字盤のデザインは表の名前で読み書きでき、どれも自分の種類の一覧に 1 度だけ載っている。
    #[test]
    fn wrist_designs_round_trip_and_belong_to_one_face() {
        assert_eq!(parse("").unwrap().wrist_design, WristDesign::Dots);
        let mut seen = Vec::new();
        for &face in WristFace::ALL {
            assert!(face.designs().len() >= 3, "{face:?}: 種類ごとに 3 つ以上");
            for &design in face.designs() {
                assert_eq!(design.face(), face);
                let text = format!("wrist_design = \"{}\"", design.key());
                assert_eq!(parse(&text).unwrap().wrist_design, design);
                assert_eq!(value(design).as_str(), Some(design.key()));
                assert!(!seen.contains(&design.key()), "{} が重複", design.key());
                seen.push(design.key());
            }
        }
    }

    #[test]
    fn mirror_frame_rate_and_keyboard_parse() {
        use subsystems::keyboard::KeyboardType;
        let c = parse("").unwrap();
        assert_eq!(
            (c.mirror_frame_rate, c.keyboard),
            (None, KeyboardType::Full)
        );
        let c = parse("mirror_frame_rate = 30\nkeyboard = \"tenkeyless\"").unwrap();
        assert_eq!(
            (c.mirror_frame_rate, c.keyboard),
            (Some(30), KeyboardType::Tenkeyless)
        );
        assert!(parse("keyboard = \"split\"").is_err());
        assert_eq!(parse("").unwrap().block_input, None);
        assert_eq!(
            parse("block_input = false").unwrap().block_input,
            Some(false)
        );
    }

    /// アクセシビリティの 3 つは独立したキーで、どれも省略でき、書き戻しても互いを消さない。
    #[test]
    fn accessibility_keys_parse_clamp_and_round_trip() {
        let c = parse("").unwrap();
        assert_eq!((c.text_scale, c.ui_scale, c.bold_text), (None, None, false));
        let c = parse("text_scale = 1.3\nui_scale = 0.8\nbold_text = true").unwrap();
        assert_eq!(
            (c.text_scale, c.ui_scale, c.bold_text),
            (Some(1.3), Some(0.8), true)
        );
        assert!(parse("bold_text = \"yes\"").is_err());
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
        save_at(&path, "text_scale", value(1.5f32)).unwrap();
        save_at(&path, "ui_scale", value(0.7f32)).unwrap();
        save_at(&path, "bold_text", value(true)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (back.text_scale, back.ui_scale, back.bold_text),
            (Some(1.5), Some(0.7), true)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// キーの音の 2 つは独立したキーで、どちらも省略でき、書き戻しても互いを消さない。
    #[test]
    fn key_sound_keys_parse_and_round_trip() {
        let c = parse("").unwrap();
        assert_eq!((c.key_sound, c.key_sound_volume), (None, None));
        let c = parse("key_sound = false\nkey_sound_volume = 0.4").unwrap();
        assert_eq!((c.key_sound, c.key_sound_volume), (Some(false), Some(0.4)));
        assert!(parse("key_sound = \"off\"").is_err());
        // 音量は 0〜1 の刻みに収まる（数でない値は最大）。
        let volume = |v| clamp_scale(v, 0.0, 1.0);
        assert_eq!(
            [volume(-1.0), volume(7.0), volume(f32::NAN)],
            [0.0, 1.0, 1.0]
        );
        assert!((volume(0.44) - 0.4).abs() < 1e-6);
        let dir = std::env::temp_dir().join(format!("nyaw-key-sound-{}", std::process::id()));
        let path = dir.join("config.toml");
        save_at(&path, "key_sound", value(false)).unwrap();
        save_at(&path, "key_sound_volume", value(0.3f32)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (back.key_sound, back.key_sound_volume),
            (Some(false), Some(0.3))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// ダッシュボードの距離・高さはどちらも省略でき、書き戻しても互いを消さない。`snap` は範囲と刻みに収める。
    #[test]
    fn dashboard_keys_parse_round_trip_and_snap() {
        let c = parse("").unwrap();
        assert_eq!((c.dashboard_distance, c.dashboard_height), (None, None));
        let c = parse("dashboard_distance = 1.5\ndashboard_height = -0.2").unwrap();
        assert_eq!(
            (c.dashboard_distance, c.dashboard_height),
            (Some(1.5), Some(-0.2))
        );
        let (min, max) = (DASHBOARD_HEIGHT_MIN, DASHBOARD_HEIGHT_MAX);
        let height = |v| snap(v, min, max, DASHBOARD_STEP, 0.0);
        assert_eq!(
            [height(-9.0), height(9.0), height(f32::NAN)],
            [min, max, 0.0]
        );
        assert!((height(0.12) - 0.1).abs() < 1e-6);
        // 0 の近くの負の値は -0 にせず 0 にする（表示が「-0.00」にならない）。
        assert!(height(-0.01).is_sign_positive());
        let dir = std::env::temp_dir().join(format!("nyaw-dashboard-{}", std::process::id()));
        let path = dir.join("config.toml");
        save_at(&path, "dashboard_distance", value(0.8f32)).unwrap();
        save_at(&path, "dashboard_height", value(0.25f32)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (back.dashboard_distance, back.dashboard_height),
            (Some(0.8), Some(0.25))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// ポインタの画面のキー（調整のしかた・カーソルの大きさ・ツールチップ・タップ音・ホバー音）はどれも省略でき、書き戻しても互いを消さない。
    #[test]
    fn pointer_keys_parse_and_round_trip() {
        let c = parse("").unwrap();
        assert_eq!(c.pointer_tuning, PointerTuning::Simple);
        assert_eq!(
            (c.cursor_scale, c.tooltips, c.tap_sound, c.hover_sound),
            (None, None, None, None)
        );
        let c = parse(
            "pointer_tuning = \"detailed\"\ncursor_scale = 1.5\ntooltips = false\n\
             tap_sound = false\nhover_sound = true",
        )
        .unwrap();
        assert_eq!(c.pointer_tuning, PointerTuning::Detailed);
        assert_eq!(
            (c.cursor_scale, c.tooltips, c.tap_sound, c.hover_sound),
            (Some(1.5), Some(false), Some(false), Some(true))
        );
        assert!(parse("pointer_tuning = \"expert\"").is_err());
        // 刻みは範囲の端にちょうど乗り、等倍は範囲の中。
        let cursor = |s| clamp_scale(s, CURSOR_SCALE_MIN, CURSOR_SCALE_MAX);
        assert_eq!(
            [cursor(0.0), cursor(9.0)],
            [CURSOR_SCALE_MIN, CURSOR_SCALE_MAX]
        );
        const { assert!(CURSOR_SCALE_MIN < 1.0 && 1.0 < CURSOR_SCALE_MAX) };
        let dir = std::env::temp_dir().join(format!("nyaw-pointer-{}", std::process::id()));
        let path = dir.join("config.toml");
        save_at(&path, "pointer_tuning", value(PointerTuning::Detailed)).unwrap();
        save_at(&path, "cursor_scale", value(0.5f32)).unwrap();
        save_at(&path, "tooltips", value(false)).unwrap();
        save_at(&path, "tap_sound", value(false)).unwrap();
        save_at(&path, "hover_sound", value(false)).unwrap();
        let back = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.pointer_tuning, PointerTuning::Detailed);
        assert_eq!(
            (
                back.cursor_scale,
                back.tooltips,
                back.tap_sound,
                back.hover_sound
            ),
            (Some(0.5), Some(false), Some(false), Some(false))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_theme_keeps_other_keys_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("nyaw-config-{}", std::process::id()));
        let path = dir.join("nested").join("config.toml");
        // ファイルもディレクトリも無くても作る。
        save_at(&path, "theme", value(ThemeMode::Light)).unwrap();
        assert_eq!(
            parse(&fs::read_to_string(&path).unwrap()).unwrap().theme,
            ThemeMode::Light
        );
        fs::write(
            &path,
            "log = \"debug\"
[gpu]
emulator = false
",
        )
        .unwrap();
        save_at(&path, "theme", value(ThemeMode::Light)).unwrap();
        let c = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (c.theme, c.log.as_deref(), c.gpu.emulator),
            (ThemeMode::Light, Some("debug"), false)
        );
        // 手ブレ抑えも同じく書き戻せ、テーマは残る。
        save_at(&path, "pointer_smoothing", 0.5).unwrap();
        let c = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (c.pointer_smoothing, c.theme),
            (Some(0.5), ThemeMode::Light)
        );
        // 言語も同じく書き戻せ、テーマは残る。
        save_at(&path, "language", value(Language::Ja)).unwrap();
        let c = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            (c.language, c.theme),
            (Some(Language::Ja), ThemeMode::Light)
        );
        // 壊れたファイルは上書きしない。
        fs::write(&path, "log = ").unwrap();
        assert!(save_at(&path, "theme", value(ThemeMode::Dark)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "log = ");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_is_config_toml_under_config_dir() {
        assert_eq!(path(), os::config_dir().map(|d| d.join("config.toml")));
    }
}
