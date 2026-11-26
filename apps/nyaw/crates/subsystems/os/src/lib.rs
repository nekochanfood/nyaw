//! OS サブシステム。保存先・ランタイム検出・タイマー精度・GPU 優先度を提供する（Windows / Linux）。
//! OS 固有の実装はバックエンドに閉じ、上位層へ公開しない。

use std::{fs, io, path::Path};

pub mod keyboard;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{
    attach_console, config_dir, data_dir, error_dialog, high_gpu_priority, log_dir,
    openvr_paths_file, openxr_runtime_manifest, precise_timers, ui_language,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{
    attach_console, config_dir, data_dir, data_search_dirs, error_dialog, high_gpu_priority,
    log_dir, openvr_paths_file, openxr_runtime_manifest, precise_timers, ui_language,
};

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("no OS backend is implemented for this target");

/// `path` に `contents` を書き込む（親ディレクトリが存在しなければ作成する）。内容が同一であれば書き込まない。
pub fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if fs::read_to_string(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
}
