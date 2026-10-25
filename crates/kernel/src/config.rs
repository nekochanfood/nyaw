//! カーネル内の設定: 設定ファイル。`os::config_dir()/config.toml`（現在のバックエンドでは `%LOCALAPPDATA%\nyaw`）を読む。
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

/// 利き手（`right` / `left`）。利き手でない側の手首に wrist を付け、配信のパネルは利き手で操作する。
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

/// `config.toml` の中身。どのキーも省略でき、知らないキーは無視する。
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 距離 LOD の品質（0 = Off、1 = Low、2 = Mid、3 = High）。省略で 2。
    pub lod_quality: LodQuality,
    /// ログレベル（`trace` / `debug` / `info` / `warn` / `error`）。解釈は呼び出し側。
    pub log: Option<String>,
    /// 使うランタイム（`openvr` / `openxr` / `novr`）。解釈は呼び出し側。
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
    /// 機能ごとに GPU を使うか（`[gpu]` の表。互換性のため CPU に落とせる）。
    pub gpu: Gpu,
    /// 見た目のテーマ（`dark` / `light`）。省略で `dark`。画面での切り替えはここへ書き戻される。
    pub theme: ThemeMode,
    /// 画面の言語（`en` / `ja`）。省略で `en`。画面での切り替えはここへ書き戻される。
    pub language: Language,
    /// 利き手（`right` / `left`）。省略で `right`。画面での切り替えはここへ書き戻される。
    pub dominant_hand: DominantHand,
    /// ミラーリング（デスクトップの配信）の取得の上限 FPS（1〜240）。省略で `desktop::DEFAULT_FRAME_RATE`。
    pub mirror_frame_rate: Option<u32>,
    /// 仮想キーボードの種類（`full` / `tenkeyless` / `compact`）。省略で `full`。
    pub keyboard: subsystems::keyboard::KeyboardType,
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
        assert_eq!(parse("").unwrap().language, Language::En);
        assert_eq!(parse("language = \"ja\"").unwrap().language, Language::Ja);
        assert!(parse("language = \"fr\"").is_err());
        assert_eq!(ThemeMode::Dark.toggled(), ThemeMode::Light);
        assert_eq!(parse("").unwrap().dominant_hand, DominantHand::Right);
        let left = parse("dominant_hand = \"left\"").unwrap().dominant_hand;
        assert_eq!((left.hand(), left.off_hand()), (vr::Hand::Left, vr::Hand::Right));
        assert!(parse("dominant_hand = \"both\"").is_err());
    }

    #[test]
    fn mirror_frame_rate_and_keyboard_parse() {
        use subsystems::keyboard::KeyboardType;
        let c = parse("").unwrap();
        assert_eq!((c.mirror_frame_rate, c.keyboard), (None, KeyboardType::Full));
        let c = parse("mirror_frame_rate = 30\nkeyboard = \"tenkeyless\"").unwrap();
        assert_eq!((c.mirror_frame_rate, c.keyboard), (Some(30), KeyboardType::Tenkeyless));
        assert!(parse("keyboard = \"split\"").is_err());
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
        assert_eq!((c.language, c.theme), (Language::Ja, ThemeMode::Light));
        // 壊れたファイルは上書きしない。
        fs::write(&path, "log = ").unwrap();
        assert!(save_at(&path, "theme", value(ThemeMode::Dark)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "log = ");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_is_config_toml_under_config_dir() {
        assert!(
            path()
                .unwrap()
                .ends_with(std::path::Path::new("nyaw").join("config.toml"))
        );
    }
}
