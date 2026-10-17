//! ブートローダー: ロガーを組み立て、カーネルを起こし、メインスレッドと載せる画面（wrist・ビーム）を UI 層に引き渡す。それ以外は何も置かない。

use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use kernel::{Kernel, config, subsystems::os};
use logger::{Level, Logger, StderrSink};

fn main() -> ExitCode {
    let config = config::load();
    init_logger(config.as_ref().ok().and_then(|c| c.log.clone()));
    let config = match config {
        Ok(c) => c,
        Err(e) => {
            logger::warn!("failed to read config file, starting with defaults", path = ?config::path(), error = e);
            config::Config {
                runtime: config::salvage_runtime(),
                ..Default::default()
            }
        }
    };
    let args = Args::parse(std::env::args().skip(1));
    // 優先順位: コマンドライン > 設定ファイル
    let requested = args.runtime.or(config.runtime.clone());
    let Ok(kernel) = Kernel::boot(requested.as_deref(), config) else {
        return ExitCode::FAILURE;
    };
    // ダッシュボードのデバッグボタンで開く画面は Testing、設定ボタンで開くのは Testing の設定画面（中身はダミー）。
    let mut panels = vec![wrist::spec(
        |opened| testing::spec(opened),
        |opened| testing::settings_spec(opened),
    )];
    // novr もコントローラを模すので、どのランタイムでもビームを載せる（novr のレーザーは視線と重なって見えない）。
    panels.extend(beam::specs());
    let profiler = args.profile.map(start_profiler);
    ui::run(kernel, panels, profiler);
    ExitCode::SUCCESS
}

#[derive(Debug, Default, PartialEq)]
struct Args {
    /// `--novr` / `--openxr` / `--openvr`（`--profile` 以外の最初の引数）。
    runtime: Option<String>,
    /// `--profile` で `Some(None)`（既定のダンプ先）、`--profile=PATH` で `Some(Some(PATH))`。
    profile: Option<Option<PathBuf>>,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut out = Self::default();
        for a in args {
            match a.strip_prefix("--profile") {
                Some("") => out.profile = Some(None),
                Some(rest) if rest.starts_with('=') => {
                    out.profile = Some(Some(PathBuf::from(&rest[1..])))
                }
                _ if out.runtime.is_none() => out.runtime = Some(a),
                _ => logger::warn!("ignoring unknown argument", arg = a),
            }
        }
        out
    }
}

/// プロファイラを起こす。ダンプ先は既定 `os::log_dir()/profile.csv`（前回分は `profile-prev.csv`）。
/// 作れなくても計測と表示はする。
fn start_profiler(path: Option<PathBuf>) -> profiler::Profiler {
    let path = path.or_else(|| os::log_dir().map(|d| d.join("profile.csv")));
    let out = path.as_ref().and_then(|p| match logger::create_rotated(p) {
        Ok(f) => Some(Box::new(BufWriter::new(f)) as Box<dyn Write>),
        Err(e) => {
            logger::warn!(
                "failed to create profile dump file",
                path = p.display(),
                error = e
            );
            None
        }
    });
    if let (Some(p), Some(_)) = (&path, &out) {
        logger::info!("dumping profile to CSV", path = p.display());
    }
    profiler::Profiler::new(out).unwrap_or_else(|e| {
        logger::warn!("failed to write profile CSV, display only", error = e);
        profiler::Profiler::new(None).expect("cannot fail without an output")
    })
}

/// 標準エラー出力と `os::log_dir()/nyaw.log` へ出すロガーを設定する。レベルは `NYAW_LOG` > 設定ファイルの `log`（既定 info）。
fn init_logger(from_config: Option<String>) {
    let requested = config::log_level().or(from_config);
    let level = requested.as_deref().and_then(|s| s.parse().ok());
    let path = os::log_dir().map(|d| d.join("nyaw.log"));
    let file = path.as_ref().map(logger::FileSink::create);

    let mut logger = Logger::new(level.unwrap_or(Level::Info)).sink(StderrSink);
    let mut file_error = None;
    match file {
        Some(Ok(sink)) => logger = logger.sink(sink),
        Some(Err(e)) => file_error = Some(e),
        None => {}
    }
    logger::init(logger).expect("logger is initialized once, first thing in main");

    // パニックもロガーを通して残す（ファイルにも書かれる）
    std::panic::set_hook(Box::new(|info| {
        logger::error!("panic", info = info);
        logger::global().flush();
    }));

    if let (Some(raw), None) = (&requested, level) {
        logger::warn!("unrecognized log level, using info", value = raw);
    }
    match (path, file_error) {
        (Some(p), None) => logger::debug!("also logging to file", path = p.display()),
        (Some(p), Some(e)) => logger::warn!(
            "failed to open log file, using stderr only",
            path = p.display(),
            error = e
        ),
        (None, _) => logger::warn!("no log file location, using stderr only"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn profile_flag_is_off_by_default_and_independent_of_runtime() {
        assert_eq!(parse(&[]), Args::default());
        assert_eq!(
            parse(&["--novr"]),
            Args {
                runtime: Some("--novr".into()),
                profile: None
            }
        );
        assert_eq!(
            parse(&["--profile", "--openxr"]),
            Args {
                runtime: Some("--openxr".into()),
                profile: Some(None)
            }
        );
        assert_eq!(
            parse(&["--openvr", "--profile=out/p.csv"]),
            Args {
                runtime: Some("--openvr".into()),
                profile: Some(Some(PathBuf::from("out/p.csv")))
            }
        );
    }
}
