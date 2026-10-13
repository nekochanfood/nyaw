//! Layer 0: OS との対話。プラットフォーム依存のもの（パス、環境変数、レジストリ等）はすべてここに置き、
//! 上位層が `#[cfg(windows)]` を書かずに済むようにする。

use std::path::PathBuf;

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
}
