//! Linux の保存先・ランタイム検出・タイマー・GPU 設定。保存先は XDG Base Directory に従う。
use std::io;
use std::path::{Path, PathBuf};

/// Linux の `sleep` は hrtimer によりナノ秒単位の精度を持つため、タイマーの精度は変更しない。
pub fn precise_timers() {}

/// OS の表示言語（ISO 639-1 の 2 文字）。対応する言語（`ja` / `en`）以外は `None`。
/// 環境変数を glibc と同じ優先順位（`LC_ALL` > `LC_MESSAGES` > `LANG`）で参照し、最初に設定されている値を用いる。
pub fn ui_language() -> Option<&'static str> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))?;
    primary_language(&locale)
}

/// ロケール名（`ja_JP.UTF-8` など）の言語部分。地域と文字コードは参照しない。
fn primary_language(locale: &str) -> Option<&'static str> {
    let language = locale.split(['_', '.', '@', '-']).next()?;
    match language.to_ascii_lowercase().as_str() {
        "ja" => Some("ja"),
        "en" => Some("en"),
        _ => None,
    }
}

/// Linux には GPU のスケジューリング優先度をプロセス単位で引き上げる汎用の API がない（Vulkan のキューの優先度は
/// デバイスの生成時に指定する）。常に `false` を返す。
// ponytail: VK_EXT_global_priority / VK_KHR_global_priority を使えばキューの優先度を上げられるが、wgpu にはこれを指定する手段がない。
pub fn high_gpu_priority() -> bool {
    false
}

/// 端末から起動された Linux のプロセスは、標準エラー出力を端末から引き継ぐ。そのため何も行わない。
pub fn attach_console() {}

/// エラーのダイアログを zenity または kdialog で表示し、閉じられるまで待機する。どちらも利用できなければログに記録する。
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
        // ダイアログを表示できない場合は、ログ（標準エラー出力とファイル）にのみ記録する。
        logger::error!("cannot show an error dialog", title = title, text = text);
    }
}

/// このアプリを次回起動するときに指定するパス。AppImage の実行ファイルは一時的なマウント先にあり、終了すると
/// 参照できなくなるため、AppImage ファイル（`APPIMAGE`）を返す。それ以外は実行ファイルを返す。
pub fn launch_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(appimage_of(&exe).unwrap_or(exe))
}

/// 実行中の nyaw を収める AppImage ファイル。AppImage から起動していなければ `None`。
pub fn running_appimage() -> Option<PathBuf> {
    appimage_of(&std::env::current_exe().ok()?)
}

fn appimage_of(exe: &Path) -> Option<PathBuf> {
    containing_appimage(exe, env_path("APPIMAGE"), env_path("APPDIR"))
}

/// `exe` を収める AppImage。2 つの環境変数は子プロセスにも引き継がれるため、`exe` が `appdir` の下にある場合に限る。
fn containing_appimage(
    exe: &Path,
    appimage: Option<PathBuf>,
    appdir: Option<PathBuf>,
) -> Option<PathBuf> {
    let appdir = appdir?;
    // `current_exe` はシンボリックリンクを解決したパスを返すため、`appdir` も解決して比較する。
    let appdir = appdir.canonicalize().unwrap_or(appdir);
    appimage.filter(|_| exe.starts_with(appdir))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        // XDG Base Directory の仕様に従い、相対パスは無効として扱う。
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

/// `$XDG_STATE_HOME`（既定 `~/.local/state`）。ログの保存先に使用する。
fn xdg_state_home() -> Option<PathBuf> {
    env_path("XDG_STATE_HOME").or_else(|| Some(home()?.join(".local").join("state")))
}

/// 共有データ（`.desktop` ファイルやアイコン）の検索先。優先度の高い順に、`$XDG_DATA_HOME`、
/// `$XDG_DATA_DIRS`（既定 `/usr/local/share:/usr/share`）を返す。
pub fn data_search_dirs() -> Vec<PathBuf> {
    let system = std::env::var("XDG_DATA_DIRS").ok();
    search_dirs(xdg_data_home(), system.as_deref())
}

fn search_dirs(user: Option<PathBuf>, system: Option<&str>) -> Vec<PathBuf> {
    let system = system
        .filter(|v| !v.is_empty())
        .unwrap_or("/usr/local/share:/usr/share");
    user.into_iter()
        .chain(
            system
                .split(':')
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
        )
        .collect()
}

/// nyaw 自身の設定の保存先: `$XDG_CONFIG_HOME/nyaw`。開発版（debug ビルド）は、開発版で変更した設定や登録用のファイルが
/// インストールしたリリース版のものと競合しないよう、`nyaw-dev` に分離する。
pub fn config_dir() -> Option<PathBuf> {
    let name = if cfg!(debug_assertions) {
        "nyaw-dev"
    } else {
        "nyaw"
    };
    Some(xdg_config_home()?.join(name))
}

/// ユーザーのデータの保存先: `$XDG_DATA_HOME/nyaw`。
pub fn data_dir() -> Option<PathBuf> {
    Some(xdg_data_home()?.join("nyaw"))
}

/// ログファイルの保存先: `$XDG_STATE_HOME/nyaw/logs`。
pub fn log_dir() -> Option<PathBuf> {
    Some(xdg_state_home()?.join("nyaw").join("logs"))
}

/// SteamVR はインストール時に `$XDG_CONFIG_HOME/openvr/openvrpaths.vrpath` を書き出す。存在すれば OpenVR を利用できる。
pub fn openvr_paths_file() -> Option<PathBuf> {
    Some(xdg_config_home()?.join("openvr").join("openvrpaths.vrpath")).filter(|p| p.exists())
}

/// アクティブな OpenXR ランタイムのマニフェスト（設定されていなければ `None`）。OpenXR ローダーと同じ順序
/// （`XR_RUNTIME_JSON` > `$XDG_CONFIG_HOME` > `$XDG_CONFIG_DIRS`（既定 `/etc/xdg`） > `/etc`）で探索する。
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
        assert_eq!(primary_language("en_GB"), Some("en"), "地域は参照しない");
        assert_eq!(primary_language("ja"), Some("ja"));
        assert_eq!(primary_language("fr_FR.UTF-8"), None, "非対応の言語");
        assert_eq!(primary_language("C"), None);
        assert_eq!(primary_language("C.UTF-8"), None);
    }

    #[test]
    fn the_users_data_directory_comes_before_the_system_ones() {
        let user = Some(PathBuf::from("/home/u/.local/share"));
        assert_eq!(
            search_dirs(user.clone(), Some("/opt/share:relative::/usr/share")),
            ["/home/u/.local/share", "/opt/share", "/usr/share"].map(PathBuf::from),
            "相対パスと空の要素は無視する"
        );
        for unset in [None, Some("")] {
            assert_eq!(
                search_dirs(user.clone(), unset),
                ["/home/u/.local/share", "/usr/local/share", "/usr/share"].map(PathBuf::from)
            );
        }
    }

    #[test]
    fn an_appimage_is_launched_by_its_own_file() {
        let appimage = Some(PathBuf::from("/home/u/nyaw.AppImage"));
        let appdir = Some(PathBuf::from("/tmp/.mount_nyawXXXX"));
        let inside = Path::new("/tmp/.mount_nyawXXXX/usr/bin/nyaw");
        assert_eq!(
            containing_appimage(inside, appimage.clone(), appdir.clone()),
            appimage
        );
        assert_eq!(
            containing_appimage(Path::new("/usr/bin/nyaw"), appimage.clone(), appdir.clone()),
            None,
            "別の AppImage から起動された場合は、引き継いだ環境変数を使わない"
        );
        assert_eq!(containing_appimage(inside, appimage, None), None);
        assert_eq!(containing_appimage(inside, None, appdir), None);
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
