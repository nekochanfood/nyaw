//! ファイルへのログ出力およびログ・計測ダンプの世代ローテーション。

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// ログをファイルへ書き出すシンク。1 行ずつ書き切るため、クラッシュ時もそこまでのログが保全される。
/// 起動ごとに新規ファイルを作成し、前回のログは 1 世代のみ `<名前>-prev.<拡張子>` に保持する。
pub struct FileSink {
    file: Mutex<File>,
    path: PathBuf,
}

impl FileSink {
    /// [`create_rotated`] で新規ファイルを作成する（前回分は 1 世代のみ保持される）。
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let file = create_rotated(path)?;
        Ok(Self {
            file: Mutex::new(file),
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 新規ファイルを作成する（ログ・プロファイラダンプ共通）。親ディレクトリが存在しなければ作成し、
/// `path` が既存なら [`prev_log_path`] へ 1 世代のみ退避する（既存の退避先は事前に削除する。Windows の rename は上書き不可のため）。
pub fn create_rotated(path: impl AsRef<Path>) -> io::Result<File> {
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
    File::create(path)
}

/// 前回ログの退避先パス: `logs/nyaw.log` → `logs/nyaw-prev.log`（拡張子なしの場合は `nyaw-prev`）。
pub fn prev_log_path(path: &Path) -> PathBuf {
    let mut name = path.file_stem().unwrap_or_default().to_os_string();
    name.push("-prev");
    if let Some(ext) = path.extension() {
        name.push(".");
        name.push(ext);
    }
    path.with_file_name(name)
}

impl crate::Sink for FileSink {
    fn write(&self, line: &str) {
        let mut buf = String::with_capacity(line.len() + 1);
        buf.push_str(line);
        buf.push('\n');
        // 書き込み失敗時もアプリは停止しない（ロガーの失敗をロガーで報告すると再帰するため無視する）。
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

#[cfg(test)]
mod tests {
    use super::*;
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
        use crate::Sink;
        let dir = std::env::temp_dir().join(format!("nyaw-os-test-{}", std::process::id()));
        let path = dir.join("a").join("nyaw.log");
        let prev = dir.join("a").join("nyaw-prev.log");
        let _ = fs::remove_dir_all(&dir);

        // 1 回目: 親ディレクトリごと作成する。退避対象は存在しない
        FileSink::create(&path).unwrap().write("run1");
        assert!(!prev.exists());

        // 2 回目: 前回分が prev に退避され、新規ファイルは空の状態で開始する
        let sink = FileSink::create(&path).unwrap();
        sink.write("run2-a");
        sink.write("run2-b");
        sink.flush();
        drop(sink);
        assert_eq!(fs::read_to_string(&path).unwrap(), "run2-a\nrun2-b\n");
        assert_eq!(fs::read_to_string(&prev).unwrap(), "run1\n");

        // 3 回目: 既存の prev は削除され、1 世代のみ保持される
        FileSink::create(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        assert_eq!(fs::read_to_string(&prev).unwrap(), "run2-a\nrun2-b\n");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn create_rotated_keeps_previous_dump() {
        let dir = std::env::temp_dir().join(format!("nyaw-os-rotate-{}", std::process::id()));
        let path = dir.join("profile.csv");
        let _ = fs::remove_dir_all(&dir);

        create_rotated(&path).unwrap().write_all(b"run1").unwrap();
        create_rotated(&path).unwrap().write_all(b"run2").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "run2");
        assert_eq!(
            fs::read_to_string(dir.join("profile-prev.csv")).unwrap(),
            "run1"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
