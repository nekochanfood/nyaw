//! カーネル内の設定: 設定ファイル。`os::config_dir()/config.toml`（Windows `%APPDATA%\nyaw`、Linux `$XDG_CONFIG_HOME/nyaw`）を読む。
//! ファイルが無ければ既定値。優先順位（コマンドライン > 環境変数 > ここ）は呼び出し側が決める。

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

/// `config.toml` の中身。どのキーも省略でき、知らないキーは無視する。
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// ログレベル（`trace` / `debug` / `info` / `warn` / `error`）。解釈は呼び出し側。
    pub log: Option<String>,
    /// 使うランタイム（`openvr` / `openxr` / `novr`）。解釈は呼び出し側。
    pub runtime: Option<String>,
    /// レーザーの手ブレ抑え（0 = 抑えない 〜 1 未満。大きいほど強く、遅れる）。省略で既定値。
    pub pointer_smoothing: Option<f32>,
    /// レーザーのエミュレート方式（`mouse` / `touch`）。省略でタッチ。
    pub pointer_mode: Option<PointerMode>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_keys_and_ignores_unknown() {
        let c = parse("log = \"debug\"\nruntime = \"novr\"\npointer_smoothing = 0.5\nfuture = 1\n")
            .unwrap();
        assert_eq!(c.pointer_smoothing, Some(0.5));
        assert_eq!(c.log.as_deref(), Some("debug"));
        assert_eq!(c.runtime.as_deref(), Some("novr"));
    }

    #[test]
    fn empty_is_default_and_bad_toml_is_error() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert!(parse("log = ").is_err());
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
