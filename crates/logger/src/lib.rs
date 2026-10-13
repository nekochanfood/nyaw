//! 横断: logfmt 形式の構造化ロガー。nyaw のすべてのクレートはログをここを通して出す（`println!` / `eprintln!` は使わない）。
//!
//! ```
//! let runtime = "NoVr";
//! logger::info!("ランタイムを選択", runtime = runtime, fallback = true);
//! // ts=2026-10-13T09:00:00.123Z level=info target=nyaw msg=ランタイムを選択 runtime=NoVr fallback=true
//! ```
//!
//! 出力先（[`Sink`]）の組み立てはブートローダー（`main`）だけが行う。ファイル出力は OS 層（`os::FileSink`）が実装する。
//! 外部クレートが `log` クレートで出すログ（gpui など）も [`init`] 以降は同じロガーへ流れる。

use std::fmt;
use std::io::{IsTerminal, Write};
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLevelError(pub String);

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown log level: {:?}", self.0)
    }
}

impl std::error::Error for ParseLevelError {}

impl FromStr for Level {
    type Err = ParseLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trace" => Ok(Level::Trace),
            "debug" => Ok(Level::Debug),
            "info" => Ok(Level::Info),
            "warn" | "warning" => Ok(Level::Warn),
            "error" => Ok(Level::Error),
            _ => Err(ParseLevelError(s.to_string())),
        }
    }
}

/// 1 件のログ。`fields` は書いた順に出力される。
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub level: Level,
    /// 発生元。マクロ経由なら `module_path!()`、`log` ブリッジ経由なら `log::Record::target()`。
    pub target: &'a str,
    pub msg: &'a str,
    pub fields: &'a [(&'a str, String)],
}

/// ログの出力先。`line` は改行を含まない 1 行の logfmt。
pub trait Sink: Send + Sync {
    fn write(&self, line: &str);
    /// 表示用シンクはレコードを整形できる。既定では保存用の logfmt を書く。
    fn write_record(&self, _record: &Record, _ts: SystemTime, line: &str) {
        self.write(line);
    }
    fn flush(&self) {}
}

/// 標準エラー出力へ書くシンク。端末では現地時刻と列揃え、リダイレクト時は logfmt。
/// 色は `NO_COLOR` が空でなく設定されている場合、または `TERM=dumb` の場合に無効。
pub struct StderrSink;

impl Sink for StderrSink {
    fn write(&self, line: &str) {
        let _ = writeln!(std::io::stderr().lock(), "{line}");
    }

    fn write_record(&self, record: &Record, ts: SystemTime, line: &str) {
        if std::io::stderr().is_terminal() {
            let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                && std::env::var_os("TERM").is_none_or(|v| v != "dumb");
            self.write(&format_terminal(record, ts, color));
        } else {
            self.write(line);
        }
    }
}

pub struct Logger {
    min: Level,
    sinks: Vec<Box<dyn Sink>>,
}

impl Logger {
    /// `min` 未満のレベルは捨てる。シンクが無ければ何も出さない。
    pub fn new(min: Level) -> Self {
        Self {
            min,
            sinks: Vec::new(),
        }
    }

    pub fn sink(mut self, sink: impl Sink + 'static) -> Self {
        self.sinks.push(Box::new(sink));
        self
    }

    pub fn min_level(&self) -> Level {
        self.min
    }

    pub fn enabled(&self, level: Level) -> bool {
        level >= self.min
    }

    pub fn log(&self, record: &Record) {
        if !self.enabled(record.level) {
            return;
        }
        let ts = SystemTime::now();
        let line = format(record, ts);
        for s in &self.sinks {
            s.write_record(record, ts, &line);
        }
    }

    pub fn flush(&self) {
        for s in &self.sinks {
            s.flush();
        }
    }
}

static GLOBAL: OnceLock<Logger> = OnceLock::new();

/// 既に初期化済み（[`init`] 済み、または初期化前にログを出してフォールバックが使われた）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyInitialized;

impl fmt::Display for AlreadyInitialized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("logger is already initialized")
    }
}

impl std::error::Error for AlreadyInitialized {}

/// グローバルロガーを設定し、`log` クレートのログも同じロガーへ流す。プロセスで 1 回だけ。
pub fn init(logger: Logger) -> Result<(), AlreadyInitialized> {
    let min = logger.min;
    GLOBAL.set(logger).map_err(|_| AlreadyInitialized)?;
    // 他で `log` のロガーが設定済みならそちらを優先する（ブリッジが無くても自前のログは出る）。
    if log::set_logger(&BRIDGE).is_ok() {
        log::set_max_level(to_log_filter(min));
    }
    Ok(())
}

/// グローバルロガー。[`init`] 前は標準エラー出力・Info 以上にフォールバックする（テストでもログが消えない）。
pub fn global() -> &'static Logger {
    GLOBAL.get_or_init(|| Logger::new(Level::Info).sink(StderrSink))
}

/// マクロ用。フィールドを組み立てる前にレベルで弾く。
#[doc(hidden)]
pub fn enabled(level: Level) -> bool {
    global().enabled(level)
}

/// マクロ用。
#[doc(hidden)]
pub fn dispatch(record: &Record) {
    global().log(record);
}

/// `level=info msg=...` のような logfmt の 1 行を作る。先頭は常に `ts level target msg`、続けて `fields` を順に。
pub fn format(record: &Record, ts: SystemTime) -> String {
    let mut out = String::with_capacity(96);
    out.push_str("ts=");
    out.push_str(&rfc3339_millis(ts));
    out.push_str(" level=");
    out.push_str(record.level.as_str());
    push_pair(&mut out, "target", record.target);
    push_pair(&mut out, "msg", record.msg);
    for (k, v) in record.fields {
        push_pair(&mut out, k, v);
    }
    out
}

fn format_terminal(record: &Record, ts: SystemTime, color: bool) -> String {
    let local: chrono::DateTime<chrono::Local> = ts.into();
    let level = std::format!("{:5}", record.level.as_str().to_ascii_uppercase());
    let level = if color {
        let code = match record.level {
            Level::Trace => 90,
            Level::Debug => 36,
            Level::Info => 32,
            Level::Warn => 33,
            Level::Error => 31,
        };
        std::format!("\x1b[{code}m{level}\x1b[0m")
    } else {
        level
    };
    let mut out = std::format!("{}  {level}  ", local.format("%Y-%m-%d %H:%M:%S%.3f %:z"));
    let mut target = String::new();
    push_value(&mut target, record.target);
    out.push_str(&std::format!("{target:24}  "));
    push_value(&mut out, record.msg);
    for (k, v) in record.fields {
        push_pair(&mut out, k, v);
    }
    out
}

fn push_pair(out: &mut String, key: &str, value: &str) {
    out.push(' ');
    out.push_str(key);
    out.push('=');
    push_value(out, value);
}

/// 空・空白・`=`・`"`・制御文字を含む値は引用してエスケープする。それ以外（日本語を含む）はそのまま。
fn push_value(out: &mut String, value: &str) {
    let needs_quote = value.is_empty()
        || value
            .chars()
            .any(|c| c == ' ' || c == '=' || c == '"' || c == '\\' || c.is_control());
    if !needs_quote {
        out.push_str(value);
        return;
    }
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&std::format!("\\u{{{:04x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// UTC の RFC 3339（ミリ秒）。`2026-10-13T09:00:00.123Z`。エポック以前は 1970-01-01 に丸める。
fn rfc3339_millis(ts: SystemTime) -> String {
    let d = ts.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (days, sod) = ((secs / 86_400) as i64, secs % 86_400);
    let (y, m, day) = civil_from_days(days);
    std::format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        sod / 3600,
        sod / 60 % 60,
        sod % 60,
        d.subsec_millis()
    )
}

/// 1970-01-01 からの日数 → (年, 月, 日)。Howard Hinnant の days_from_civil の逆変換。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

// ---- log クレートからの橋渡し ----

struct Bridge;

static BRIDGE: Bridge = Bridge;

impl log::Log for Bridge {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        enabled(from_log_level(metadata.level()))
    }

    fn log(&self, record: &log::Record) {
        let level = from_log_level(record.level());
        if !enabled(level) {
            return;
        }
        let msg = record.args().to_string();
        dispatch(&Record {
            level,
            target: record.target(),
            msg: &msg,
            fields: &[],
        });
    }

    fn flush(&self) {
        global().flush();
    }
}

fn from_log_level(l: log::Level) -> Level {
    match l {
        log::Level::Trace => Level::Trace,
        log::Level::Debug => Level::Debug,
        log::Level::Info => Level::Info,
        log::Level::Warn => Level::Warn,
        log::Level::Error => Level::Error,
    }
}

fn to_log_filter(l: Level) -> log::LevelFilter {
    match l {
        Level::Trace => log::LevelFilter::Trace,
        Level::Debug => log::LevelFilter::Debug,
        Level::Info => log::LevelFilter::Info,
        Level::Warn => log::LevelFilter::Warn,
        Level::Error => log::LevelFilter::Error,
    }
}

// ---- マクロ ----

/// `logger::log!(Level::Info, "msg", key = value, key = ?debug_value)`。
/// `key = expr` は `Display`、`key = ?expr` は `Debug` で整形する。レベルで弾かれたらフィールドは評価しない。
#[macro_export]
macro_rules! log {
    ($level:expr, $msg:expr $(, $($rest:tt)*)?) => {{
        let level: $crate::Level = $level;
        if $crate::enabled(level) {
            #[allow(unused_mut)]
            let mut fields: ::std::vec::Vec<(&'static str, ::std::string::String)> =
                ::std::vec::Vec::new();
            $crate::__fields!(fields; $($($rest)*)?);
            $crate::dispatch(&$crate::Record {
                level,
                target: ::std::module_path!(),
                msg: ::std::convert::AsRef::<str>::as_ref(&$msg),
                fields: &fields,
            });
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __fields {
    ($f:ident;) => {};
    ($f:ident; $k:ident = ? $v:expr $(, $($rest:tt)*)?) => {
        $f.push((::std::stringify!($k), ::std::format!("{:?}", $v)));
        $crate::__fields!($f; $($($rest)*)?);
    };
    ($f:ident; $k:ident = $v:expr $(, $($rest:tt)*)?) => {
        $f.push((::std::stringify!($k), ::std::format!("{}", $v)));
        $crate::__fields!($f; $($($rest)*)?);
    };
}

#[macro_export]
macro_rules! trace {
    ($($t:tt)*) => { $crate::log!($crate::Level::Trace, $($t)*) };
}

#[macro_export]
macro_rules! debug {
    ($($t:tt)*) => { $crate::log!($crate::Level::Debug, $($t)*) };
}

#[macro_export]
macro_rules! info {
    ($($t:tt)*) => { $crate::log!($crate::Level::Info, $($t)*) };
}

#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => { $crate::log!($crate::Level::Warn, $($t)*) };
}

#[macro_export]
macro_rules! error {
    ($($t:tt)*) => { $crate::log!($crate::Level::Error, $($t)*) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl Sink for Capture {
        fn write(&self, line: &str) {
            self.0.lock().unwrap().push(line.to_string());
        }
    }

    fn rec<'a>(level: Level, msg: &'a str, fields: &'a [(&'a str, String)]) -> Record<'a> {
        Record {
            level,
            target: "vr",
            msg,
            fields,
        }
    }

    fn at(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    #[test]
    fn format_puts_fixed_keys_first_then_fields_in_order() {
        let fields = [("runtime", "NoVr".to_string()), ("fallback", "true".into())];
        let line = format(&rec(Level::Info, "runtime selected", &fields), at(0));
        assert_eq!(
            line,
            "ts=1970-01-01T00:00:00.000Z level=info target=vr msg=\"runtime selected\" runtime=NoVr fallback=true"
        );
    }

    #[test]
    fn values_are_quoted_and_escaped_only_when_needed() {
        let fields = [
            ("plain", "abc".to_string()),
            ("empty", String::new()),
            ("eq", "a=b".into()),
            ("quote", "say \"hi\"".into()),
            ("newline", "a\nb\tc".into()),
            ("backslash", r"C:\nyaw".into()),
            ("ja", "描画失敗".into()),
            ("ctrl", "\u{1}".into()),
        ];
        let line = format(&rec(Level::Warn, "m", &fields), at(0));
        let tail = line.split_once("msg=m ").unwrap().1;
        assert_eq!(
            tail,
            r#"plain=abc empty="" eq="a=b" quote="say \"hi\"" newline="a\nb\tc" backslash="C:\\nyaw" ja=描画失敗 ctrl="\u{0001}""#
        );
    }

    #[test]
    fn timestamp_is_utc_rfc3339_with_millis() {
        // 2026-10-13T09:08:07.654Z
        assert_eq!(
            rfc3339_millis(at(1_791_882_487_654)),
            "2026-10-13T09:08:07.654Z"
        );
        // うるう日
        assert_eq!(
            rfc3339_millis(at(951_782_400_000)),
            "2000-02-29T00:00:00.000Z"
        );
        // エポック以前は 1970-01-01 に丸める
        assert_eq!(
            rfc3339_millis(UNIX_EPOCH - Duration::from_secs(1)),
            "1970-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn terminal_uses_local_time_and_aligned_levels_with_optional_color() {
        let ts = at(1_791_882_487_654);
        let local: chrono::DateTime<chrono::Local> = ts.into();
        let timestamp = local.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string();
        for (level, label, code) in [
            (Level::Trace, "TRACE", 90),
            (Level::Debug, "DEBUG", 36),
            (Level::Info, "INFO ", 32),
            (Level::Warn, "WARN ", 33),
            (Level::Error, "ERROR", 31),
        ] {
            let record = rec(level, "描画完了", &[]);
            let plain = format_terminal(&record, ts, false);
            assert_eq!(
                plain,
                std::format!("{timestamp}  {label}  {:24}  描画完了", "vr")
            );
            let colored = format_terminal(&record, ts, true);
            assert_eq!(
                colored,
                plain.replacen(label, &std::format!("\x1b[{code}m{label}\x1b[0m"), 1)
            );
        }
    }

    #[test]
    fn terminal_escapes_control_characters_and_preserves_fields() {
        let fields = [("error", "device\nlost\x1b[31m".into())];
        let record = Record {
            target: "ui\tpanel",
            ..rec(Level::Warn, "bad\r\nmessage", &fields)
        };
        let line = format_terminal(&record, at(0), false);
        assert!(!line.contains(['\n', '\r', '\t', '\x1b']));
        assert!(line.contains(r#""ui\tpanel""#));
        assert!(line.ends_with(r#""bad\r\nmessage" error="device\nlost\u{001b}[31m""#));
    }

    #[test]
    fn level_parses_case_insensitively() {
        assert_eq!("DEBUG".parse(), Ok(Level::Debug));
        assert_eq!(" warning ".parse(), Ok(Level::Warn));
        assert!("loud".parse::<Level>().is_err());
        assert!(Level::Error > Level::Warn && Level::Trace < Level::Debug);
    }

    #[test]
    fn logger_drops_records_below_min_and_fans_out_to_every_sink() {
        let (a, b) = (Capture::default(), Capture::default());
        let logger = Logger::new(Level::Info).sink(a.clone()).sink(b.clone());
        logger.log(&rec(Level::Debug, "hidden", &[]));
        logger.log(&rec(Level::Error, "shown", &[]));
        for c in [a, b] {
            let lines = c.0.lock().unwrap();
            assert_eq!(lines.len(), 1);
            assert!(lines[0].contains("level=error target=vr msg=shown"));
        }
    }

    #[test]
    fn macro_builds_display_and_debug_fields_and_skips_disabled_levels() {
        // グローバルは未初期化なので Info 以上。Debug のフィールドは評価されない。
        let mut evaluated = false;
        crate::debug!(
            "x",
            side = {
                evaluated = true;
                1
            }
        );
        assert!(!evaluated);

        let mut fields = Vec::new();
        crate::__fields!(fields; n = 1 + 1, s = ?"a b", trailing = 'c',);
        assert_eq!(
            fields,
            [
                ("n", "2".to_string()),
                ("s", "\"a b\"".to_string()),
                ("trailing", "c".to_string())
            ]
        );
    }

    #[test]
    fn log_levels_map_both_ways() {
        for (ours, theirs) in [
            (Level::Trace, log::Level::Trace),
            (Level::Debug, log::Level::Debug),
            (Level::Info, log::Level::Info),
            (Level::Warn, log::Level::Warn),
            (Level::Error, log::Level::Error),
        ] {
            assert_eq!(from_log_level(theirs), ours);
            assert_eq!(to_log_filter(ours), theirs.to_level_filter());
        }
    }
}
