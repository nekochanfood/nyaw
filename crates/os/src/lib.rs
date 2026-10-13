//! Layer 0: OS との対話。プラットフォーム依存のもの（パス、環境変数、レジストリ、ファイル入出力等）はすべてここに置き、
//! 上位層が `#[cfg(windows)]` を書かずに済むようにする。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
}

#[cfg(windows)]
pub const CURRENT: Os = Os::Windows;
#[cfg(target_os = "linux")]
pub const CURRENT: Os = Os::Linux;
#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("nyaw supports Windows and Linux only");

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// ユーザーごとの設定ルート: Windows は `%APPDATA%`、Linux は `$XDG_CONFIG_HOME` か `~/.config`。
fn user_config_root() -> Option<PathBuf> {
    match CURRENT {
        Os::Windows => env_path("APPDATA"),
        Os::Linux => {
            env_path("XDG_CONFIG_HOME").or_else(|| env_path("HOME").map(|h| h.join(".config")))
        }
    }
}

/// nyaw 自身の設定の保存先。
pub fn config_dir() -> Option<PathBuf> {
    user_config_root().map(|p| p.join("nyaw"))
}

/// ログファイルの置き場: Windows は `%LOCALAPPDATA%\nyaw\logs`、
/// Linux は `$XDG_STATE_HOME/nyaw/logs` か `~/.local/state/nyaw/logs`。
pub fn log_dir() -> Option<PathBuf> {
    let root = match CURRENT {
        Os::Windows => env_path("LOCALAPPDATA")?,
        Os::Linux => env_path("XDG_STATE_HOME")
            .or_else(|| env_path("HOME").map(|h| h.join(".local").join("state")))?,
    };
    Some(root.join("nyaw").join("logs"))
}

/// 環境変数 `NYAW_LOG` で指定されたログレベル（`trace` / `debug` / `info` / `warn` / `error`）。解釈は呼び出し側。
pub fn log_level() -> Option<String> {
    std::env::var("NYAW_LOG").ok().filter(|v| !v.is_empty())
}

/// ログをファイルへ追記するシンク。1 行ずつ書き切るので、落ちてもそこまでのログは残る。
// ponytail: ローテーション無し（同じファイルに追記し続ける）。肥大化が問題になったら日付ごとに分ける。
pub struct FileSink {
    file: Mutex<File>,
    path: PathBuf,
}

impl FileSink {
    /// 親ディレクトリが無ければ作り、追記モードで開く。
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl logger::Sink for FileSink {
    fn write(&self, line: &str) {
        let mut buf = String::with_capacity(line.len() + 1);
        buf.push_str(line);
        buf.push('\n');
        // 書けなくてもアプリは止めない（ロガーの失敗をロガーで報告すると再帰するので黙って捨てる）。
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(buf.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

/// SteamVR はインストール時に `openvrpaths.vrpath` を書き出す。存在すれば OpenVR が使える。
pub fn openvr_paths_file() -> Option<PathBuf> {
    let dir = match CURRENT {
        Os::Windows => env_path("LOCALAPPDATA")?,
        Os::Linux => user_config_root()?,
    };
    Some(dir.join("openvr").join("openvrpaths.vrpath")).filter(|p| p.exists())
}

/// 設定されていれば、アクティブな OpenXR ランタイムのマニフェスト。
pub fn openxr_runtime_manifest() -> Option<PathBuf> {
    if let Some(p) = env_path("XR_RUNTIME_JSON") {
        return Some(p);
    }
    let candidates: Vec<PathBuf> = match CURRENT {
        // ponytail: Windows のレジストリ (HKLM\SOFTWARE\Khronos\OpenXR\1\ActiveRuntime) は未対応。
        // Windows で OpenXR 検出が必要になったらここでレジストリを読む。
        Os::Windows => vec![],
        Os::Linux => {
            let mut v: Vec<PathBuf> = user_config_root().into_iter().collect();
            v.push("/etc/xdg".into());
            v.into_iter()
                .map(|d| d.join("openxr/1/active_runtime.json"))
                .collect()
        }
    };
    candidates.into_iter().find(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_is_namespaced() {
        // CI のシェルには APPDATA / HOME が必ず設定されている。
        assert!(config_dir().unwrap().ends_with("nyaw"));
    }

    #[test]
    fn log_dir_is_namespaced() {
        assert!(log_dir().unwrap().ends_with(Path::new("nyaw").join("logs")));
    }

    #[test]
    fn file_sink_creates_parent_dirs_and_appends_lines() {
        use logger::Sink;
        let dir = std::env::temp_dir().join(format!("nyaw-os-test-{}", std::process::id()));
        let path = dir.join("a").join("b.log");
        let _ = fs::remove_dir_all(&dir);

        FileSink::open(&path).unwrap().write("first");
        let sink = FileSink::open(&path).unwrap();
        sink.write("second");
        sink.flush();

        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
