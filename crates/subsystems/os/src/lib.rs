//! OS サブシステム。保存先・ランタイム検出・タイマー精度・GPU 優先度を提供する。
//! OS 固有の実装はバックエンドに閉じ、上位層へ持ち出さない。

use std::{fs, io, path::Path};

pub mod keyboard;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{
    config_dir, data_dir, high_gpu_priority, log_dir, openvr_paths_file, openxr_runtime_manifest,
    precise_timers,
};

#[cfg(not(windows))]
compile_error!("no OS backend is implemented for this target");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
}

#[cfg(windows)]
pub const CURRENT: Os = Os::Windows;

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
