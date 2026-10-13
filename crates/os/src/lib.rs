//! Layer 0: OS との対話。プラットフォーム依存のもの（パス、環境変数、レジストリ、ファイル入出力等）はすべてここに置き、
//! 上位層が `#[cfg(windows)]` を書かずに済むようにする。

use std::fs::{self, File};
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

/// nyaw 自身の設定の保存先。ログと同じ `nyaw` ディレクトリ（`log_dir` の親）。
pub fn config_dir() -> Option<PathBuf> {
    log_dir().and_then(|d| d.parent().map(Path::to_path_buf))
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

/// ログをファイルへ書くシンク。1 行ずつ書き切るので、落ちてもそこまでのログは残る。
/// 起動ごとに新しいファイルにし、前回のログは 1 世代だけ `<名前>-prev.<拡張子>` に残す。
pub struct FileSink {
    file: Mutex<File>,
    path: PathBuf,
}

impl FileSink {
    /// 親ディレクトリが無ければ作り、新しいファイルを作る。
    /// 既に `path` があれば [`prev_log_path`] へ退避する（既存の退避先は先に消す。Windows の rename は上書きできないため）。
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        if path.exists() {
            let prev = prev_log_path(path);
            match fs::remove_file(&prev) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            fs::rename(path, &prev)?;
        }
        let file = File::create(path)?;
        Ok(Self {
            file: Mutex::new(file),
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 前回のログの退避先: `logs/nyaw.log` → `logs/nyaw-prev.log`（拡張子が無ければ `nyaw-prev`）。
pub fn prev_log_path(path: &Path) -> PathBuf {
    let mut name = path.file_stem().unwrap_or_default().to_os_string();
    name.push("-prev");
    if let Some(ext) = path.extension() {
        name.push(".");
        name.push(ext);
    }
    path.with_file_name(name)
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

/// `path` に `contents` を書く（親ディレクトリが無ければ作る）。中身が同じなら書かない。
pub fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if fs::read_to_string(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
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
        Os::Windows => active_runtime_from_registry().into_iter().collect(),
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

/// `reg query` の出力から `ActiveRuntime` の値（マニフェストのパス）を取り出す。
fn parse_reg_value(out: &str) -> Option<PathBuf> {
    let line = out.lines().find(|l| l.contains("REG_SZ"))?;
    let value = line.split_once("REG_SZ")?.1.trim();
    (!value.is_empty()).then(|| PathBuf::from(value))
}

/// HKLM\SOFTWARE\Khronos\OpenXR\1 の ActiveRuntime。
// ponytail: reg.exe を呼ぶ（依存を足さない）。毎回プロセスを起こすので、頻繁に呼ぶなら winreg にする。
fn active_runtime_from_registry() -> Option<PathBuf> {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Khronos\OpenXR\1",
            "/v",
            "ActiveRuntime",
        ])
        .output()
        .ok()?;
    parse_reg_value(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reg_query_output() {
        let out = r"
HKEY_LOCAL_MACHINE\SOFTWARE\Khronos\OpenXR\1
    ActiveRuntime    REG_SZ    C:\Program Files\SteamVR\steamxr_win64.json
";
        assert_eq!(
            parse_reg_value(out),
            Some(PathBuf::from(
                r"C:\Program Files\SteamVR\steamxr_win64.json"
            ))
        );
        assert_eq!(parse_reg_value("ERROR: not found"), None);
    }

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
    fn prev_log_path_inserts_suffix_before_extension() {
        let dir = Path::new("logs");
        assert_eq!(
            prev_log_path(&dir.join("nyaw.log")),
            dir.join("nyaw-prev.log")
        );
        assert_eq!(prev_log_path(&dir.join("nyaw")), dir.join("nyaw-prev"));
    }

    #[test]
    fn file_sink_keeps_one_previous_generation() {
        use logger::Sink;
        let dir = std::env::temp_dir().join(format!("nyaw-os-test-{}", std::process::id()));
        let path = dir.join("a").join("nyaw.log");
        let prev = dir.join("a").join("nyaw-prev.log");
        let _ = fs::remove_dir_all(&dir);

        // 1 回目: 親ディレクトリごと作る。退避するものは無い
        FileSink::create(&path).unwrap().write("run1");
        assert!(!prev.exists());

        // 2 回目: 前回分が prev に移り、新しいファイルは空から始まる
        let sink = FileSink::create(&path).unwrap();
        sink.write("run2-a");
        sink.write("run2-b");
        sink.flush();
        drop(sink);
        assert_eq!(fs::read_to_string(&path).unwrap(), "run2-a\nrun2-b\n");
        assert_eq!(fs::read_to_string(&prev).unwrap(), "run1\n");

        // 3 回目: 既存の prev は消され、1 世代だけ残る
        FileSink::create(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        assert_eq!(fs::read_to_string(&prev).unwrap(), "run2-a\nrun2-b\n");

        fs::remove_dir_all(&dir).unwrap();
    }
}
