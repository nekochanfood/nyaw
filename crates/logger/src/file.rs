//! ファイルへのログ出力と、ログ・計測ダンプの世代管理。

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// ログをファイルへ書くシンク。1 行ずつ書き切るので、落ちてもそこまでのログは残る。
/// 起動ごとに新しいファイルにし、前回のログは 1 世代だけ `<名前>-prev.<拡張子>` に残す。
pub struct FileSink {
    file: Mutex<File>,
    path: PathBuf,
}

impl FileSink {
    /// [`create_rotated`] で新しいファイルを作る（前回分は 1 世代だけ残る）。
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

/// 新しいファイルを作る（ログ・プロファイラのダンプ共通）。親ディレクトリが無ければ作り、
/// 既に `path` があれば [`prev_log_path`] へ 1 世代だけ退避する（既存の退避先は先に消す。Windows の rename は上書きできないため）。
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

impl crate::Sink for FileSink {
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
