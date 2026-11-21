//! ブートローダー: ロガーを構成し、カーネルを初期化して、メインスレッドと表示する画面（wrist・ビーム）を UI 層に引き渡す。それ以外の責務は持たない。
// リリースビルドではコンソールウィンドウを非表示にする（ログは `--console` で端末または新規コンソールに出力する）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use kernel::{Kernel, config, subsystems::os};
use logger::{Level, Logger, StderrSink};

fn main() -> ExitCode {
    // ロガーの初期化に先立ち出力先を確保する（事前に確保しないと初期ログが失われる）。
    if std::env::args().any(|a| a == "--console") {
        os::attach_console();
    }
    let config = config::load();
    init_logger(config.as_ref().ok().and_then(|c| c.logger.level.clone()));
    let config = config.unwrap_or_else(|e| {
        logger::warn!("failed to read config file, starting with defaults", path = ?config::path(), error = e);
        config::Config::default()
    });
    let args = Args::parse(std::env::args().skip(1));
    // ランタイムはコマンドラインでのみ指定する（novr は明示的な指定を必要とするため、設定ファイルには置かない）。
    let requested = args.runtime;
    // novr はテスト用。リリースビルドでは VR ランタイムが見つからなくても自動的に novr へフォールバックせず、明示的な指定を必要とする。
    let Ok(kernel) = Kernel::boot(requested.as_deref(), config, cfg!(debug_assertions)) else {
        // コンソール非接続時にもユーザーへ通知するためダイアログを使用する。回避策は表示しない（novr を一般ユーザーに案内しない）。
        os::error_dialog("nyaw", "ランタイムが見つかりませんでした");
        return ExitCode::FAILURE;
    };
    // ダッシュボードのデバッグボタンで開く画面は Testing、設定ボタンで開く画面は Testing の設定画面（内容はプレースホルダー）。
    let mut panels = vec![wrist::spec(
        &kernel.settings(),
        |from, opened| {
            testing::spec(from, move |id, cx| {
                // デバッグ画面経由でセットアップウィザードの再実行を可能にする。
                cx.set_global(testing::SetupWizardSpawn(setup::spawn_spec));
                opened(id, cx)
            })
        },
        |from, opened| settings::spec(from, opened),
    )];
    // novr もコントローラをエミュレートするため、すべてのランタイムでビームを登録する（novr のレーザーは視線と重なり視認できない）。
    panels.extend(beam::specs());
    // 初回起動時（セットアップ未完了）はセットアップウィザードを表示する。
    if !kernel.settings().setup_done {
        panels.push(setup::spec());
    }
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
            if a == "--console" {
                continue; // main の先頭で処理済み
            }
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

/// プロファイラを初期化する。出力先の既定は `os::log_dir()/profile.csv`（前回分は `profile-prev.csv`）。
/// ファイルの作成に失敗しても計測と表示は行う。
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

/// 標準エラー出力と `os::log_dir()/nyaw.log` に出力するロガーを初期化する。レベルの優先順位は `NYAW_LOG` > 設定ファイルの `logger.level`（既定 info）。
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

    // パニックもロガー経由で記録する（ファイルにも出力される）
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
        assert_eq!(parse(&["--console"]), Args::default());
    }
}
