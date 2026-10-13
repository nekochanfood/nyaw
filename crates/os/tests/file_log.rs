//! 結合テスト: logger の整形 → os の FileSink → ファイル、が logfmt の 1 行として読み戻せること。

use std::fs;

use logger::{Level, Logger, Record};
use os::FileSink;

#[test]
fn logger_writes_logfmt_lines_through_file_sink() {
    let dir = std::env::temp_dir().join(format!("nyaw-file-log-{}", std::process::id()));
    let path = dir.join("nyaw.log");
    let _ = fs::remove_dir_all(&dir);

    let logger = Logger::new(Level::Info).sink(FileSink::open(&path).unwrap());
    let fields = [("error", "device lost".to_string()), ("panel", "0".into())];
    logger.log(&Record {
        level: Level::Warn,
        target: "ui",
        msg: "パネルの描画を取得できない",
        fields: &fields,
    });
    logger.log(&Record {
        level: Level::Debug,
        target: "ui",
        msg: "捨てられる",
        fields: &[],
    });
    logger.flush();

    let text = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{text}");
    let line = lines[0];
    assert!(line.starts_with("ts="), "{line}");
    assert!(
        line.ends_with(
            r#" level=warn target=ui msg=パネルの描画を取得できない error="device lost" panel=0"#
        ),
        "{line}"
    );
    fs::remove_dir_all(&dir).unwrap();
}
