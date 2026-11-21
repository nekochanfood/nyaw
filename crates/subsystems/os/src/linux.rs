//! Linux の保存先・ランタイム検出・タイマー・GPU 設定。保存先は XDG Base Directory に従う。
use std::path::PathBuf;

/// Linux の `sleep` はもとから高精度（ナノ秒単位の hrtimer）なので何もしない。
pub fn precise_timers() {}

/// OS の表示言語（ISO 639-1 の 2 文字）。知っている言語（`ja` / `en`）でなければ `None`。
/// glibc と同じ優先順位（`LC_ALL` > `LC_MESSAGES` > `LANG`）で、最初の空でない値を読む。
pub fn ui_language() -> Option<&'static str> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))?;
    primary_language(&locale)
}

/// `ja_JP.UTF-8` のようなロケール名の言語部分。地域や文字コードは参照しない。
fn primary_language(locale: &str) -> Option<&'static str> {
    let language = locale.split(['_', '.', '@', '-']).next()?;
    match language.to_ascii_lowercase().as_str() {
        "ja" => Some("ja"),
        "en" => Some("en"),
        _ => None,
    }
}

/// GPU のプロセス優先度を上げる手段は Linux の汎用 API にはない（Vulkan のキュー優先度はデバイスごと）。常に `false`。
// ponytail: VK_EXT_global_priority / VK_KHR_global_priority でキューを高優先度にできるが、wgpu から指定する口がない。
pub fn high_gpu_priority() -> bool {
    false
}

/// Linux のプロセスは端末から起動されれば標準エラー出力をそのまま持つため、何もしない。
pub fn attach_console() {}

/// エラーのダイアログを表示し、閉じられるまで待機する。zenity / kdialog がなければ標準エラー出力に書く。
pub fn error_dialog(title: &str, text: &str) {
    let shown = [
        ("zenity", vec!["--error", "--title", title, "--text", text]),
        ("kdialog", vec!["--title", title, "--error", text]),
    ]
    .into_iter()
    .any(|(program, args)| {
        std::process::Command::new(program)
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    });
    if !shown {
        // ダイアログを出せない（zenity / kdialog がない）ときは、ログ（標準エラー出力とファイル）にだけ残す。
        logger::error!("cannot show an error dialog", title = title, text = text);
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        // XDG の仕様上、相対パスは無効として無視する。
        .filter(|p| p.is_absolute())
}

fn home() -> Option<PathBuf> {
    env_path("HOME")
}

/// `$XDG_CONFIG_HOME`（既定 `~/.config`）。
fn xdg_config_home() -> Option<PathBuf> {
    env_path("XDG_CONFIG_HOME").or_else(|| Some(home()?.join(".config")))
}

/// `$XDG_DATA_HOME`（既定 `~/.local/share`）。
fn xdg_data_home() -> Option<PathBuf> {
    env_path("XDG_DATA_HOME").or_else(|| Some(home()?.join(".local").join("share")))
}

/// `$XDG_STATE_HOME`（既定 `~/.local/state`）。ログの置き場。
fn xdg_state_home() -> Option<PathBuf> {
    env_path("XDG_STATE_HOME").or_else(|| Some(home()?.join(".local").join("state")))
}

/// nyaw 自身の設定の保存先: `$XDG_CONFIG_HOME/nyaw`。開発版（debug ビルド）は `nyaw-dev` に分離する（Windows と同じ理由）。
pub fn config_dir() -> Option<PathBuf> {
    let name = if cfg!(debug_assertions) {
        "nyaw-dev"
    } else {
        "nyaw"
    };
    Some(xdg_config_home()?.join(name))
}

/// ユーザーのデータの置き場: `$XDG_DATA_HOME/nyaw`。
pub fn data_dir() -> Option<PathBuf> {
    Some(xdg_data_home()?.join("nyaw"))
}

/// ログファイルの置き場: `$XDG_STATE_HOME/nyaw/logs`。
pub fn log_dir() -> Option<PathBuf> {
    Some(xdg_state_home()?.join("nyaw").join("logs"))
}

/// SteamVR はインストール時に `$XDG_CONFIG_HOME/openvr/openvrpaths.vrpath` を書き出す。存在すれば OpenVR が利用可能。
pub fn openvr_paths_file() -> Option<PathBuf> {
    Some(xdg_config_home()?.join("openvr").join("openvrpaths.vrpath")).filter(|p| p.exists())
}

/// 設定されていれば、アクティブな OpenXR ランタイムのマニフェスト。OpenXR ローダーと同じ順に探す
/// （`XR_RUNTIME_JSON` > `$XDG_CONFIG_HOME` > `$XDG_CONFIG_DIRS`（既定 `/etc/xdg`） > `/etc`）。
pub fn openxr_runtime_manifest() -> Option<PathBuf> {
    if let Some(p) = env_path("XR_RUNTIME_JSON") {
        return Some(p);
    }
    let config_dirs = std::env::var("XDG_CONFIG_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/etc/xdg".into());
    xdg_config_home()
        .into_iter()
        .chain(config_dirs.split(':').map(PathBuf::from))
        .chain([PathBuf::from("/etc")])
        .map(|dir| dir.join("openxr").join("1").join("active_runtime.json"))
        .find(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_maps_to_the_known_languages() {
        assert_eq!(primary_language("ja_JP.UTF-8"), Some("ja"));
        assert_eq!(primary_language("en_US.UTF-8"), Some("en"));
        assert_eq!(primary_language("en_GB"), Some("en"), "地域は見ない");
        assert_eq!(primary_language("ja"), Some("ja"));
        assert_eq!(primary_language("fr_FR.UTF-8"), None, "知らない言語");
        assert_eq!(primary_language("C"), None);
        assert_eq!(primary_language("C.UTF-8"), None);
    }

    #[test]
    fn config_dir_is_namespaced() {
        let name = if cfg!(debug_assertions) {
            "nyaw-dev"
        } else {
            "nyaw"
        };
        assert!(config_dir().unwrap().ends_with(name));
    }

    #[test]
    fn log_dir_is_namespaced() {
        assert!(
            log_dir()
                .unwrap()
                .ends_with(std::path::Path::new("nyaw").join("logs"))
        );
    }
}
