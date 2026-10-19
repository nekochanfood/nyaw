//! OS サブシステム。保存先・ランタイム検出・タイマー精度・GPU 優先度の環境差を吸収し、
//! 上位層が `#[cfg(windows)]` を書かずに済むようにする。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub mod keyboard;

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

/// OS のタイマーの精度を 1ms にする（プロセスが終わるまで有効）。Windows の既定は 15.6ms 刻みで、
/// フレームループの「1ms 譲る」待ちが 1 刻みに丸められてフレームが伸びる。Linux は元から細かいので何もしない。
pub fn precise_timers() {
    #[cfg(windows)]
    {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn timeBeginPeriod(period: u32) -> u32;
        }
        // SAFETY: 整数を渡すだけの Win32 API。
        unsafe {
            timeBeginPeriod(1);
        }
    }
}

/// このプロセスの GPU スケジューリングの優先度を上げる（プロセスが終わるまで有効。D3D11 も Vulkan も対象）。
/// 既定（通常）のままだと、ゲームが GPU を使い切っている間は小さな描画もゲームのフレームの後ろに並ばされ、
/// 読み戻しの完了待ちが 1 回 100ms 近くになってフレームループごと止まる。
/// REALTIME は管理者権限が要るので HIGH。Linux は何もしない。成功したか返す。
pub fn high_gpu_priority() -> bool {
    #[cfg(windows)]
    {
        const D3DKMT_SCHEDULINGPRIORITYCLASS_HIGH: i32 = 4;
        #[link(name = "gdi32")]
        unsafe extern "system" {
            fn D3DKMTSetProcessSchedulingPriorityClass(process: isize, priority: i32) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> isize;
        }
        // SAFETY: 自プロセスの疑似ハンドルと整数を渡すだけ。戻り値は NTSTATUS（0 が成功）。
        unsafe {
            D3DKMTSetProcessSchedulingPriorityClass(
                GetCurrentProcess(),
                D3DKMT_SCHEDULINGPRIORITYCLASS_HIGH,
            ) == 0
        }
    }
    #[cfg(not(windows))]
    true
}

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

/// ユーザーのデータ（ニャプリのパッケージとその保存領域）の置き場: Windows は `%LOCALAPPDATA%\nyaw`、
/// Linux は `$XDG_DATA_HOME/nyaw` か `~/.local/share/nyaw`。
pub fn data_dir() -> Option<PathBuf> {
    let root = match CURRENT {
        Os::Windows => env_path("LOCALAPPDATA")?,
        Os::Linux => env_path("XDG_DATA_HOME")
            .or_else(|| env_path("HOME").map(|h| h.join(".local").join("share")))?,
    };
    Some(root.join("nyaw"))
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
    fn raises_gpu_priority_without_admin() {
        assert!(high_gpu_priority());
    }
}
