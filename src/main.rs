//! ブートローダー: ロガーを組み立て、カーネルを起こし、メインスレッドと載せる画面（wrist・ビーム）を UI 層に引き渡す。それ以外は何も置かない。
//! `--webui` のときは UI 層の代わりに Web UI 層（Tauri）へ、Web の画面（Testing の Web 版・ビーム）とともに引き渡す。

use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use kernel::{Kernel, config};
use logger::{Level, Logger, StderrSink};

fn main() -> ExitCode {
    let config = config::load();
    init_logger(config.as_ref().ok().and_then(|c| c.log.clone()));
    let config = match config {
        Ok(c) => c,
        Err(e) => {
            logger::warn!("設定ファイルを読めないので既定値で起動する", path = ?config::path(), error = e);
            config::Config::default()
        }
    };
    let args = Args::parse(std::env::args().skip(1));
    // 優先順位: コマンドライン > 設定ファイル
    let requested = args.runtime.or(config.runtime.clone());
    let Ok(kernel) = Kernel::boot(requested.as_deref(), config) else {
        return ExitCode::FAILURE;
    };
    if args.webui {
        if args.profile.is_some() {
            logger::warn!("Web UI ではプロファイラを使えないので --profile を無視する");
        }
        let mut panels = vec![webtesting::spec()];
        if kernel.surface() == kernel::Surface::Overlay {
            panels.extend(webbeam::specs());
        }
        return match webui::run(kernel, panels) {
            0 => ExitCode::SUCCESS,
            _ => ExitCode::FAILURE,
        };
    }
    let mut panels = vec![wrist::spec()];
    // ビームは VR のコントローラ用。novr ではマウスがそのまま届くので要らない。
    if kernel.surface() == kernel::Surface::Overlay {
        panels.extend(beam::specs());
    }
    let profiler = args.profile.map(start_profiler);
    ui::run(kernel, panels, profiler);
    ExitCode::SUCCESS
}

#[derive(Debug, Default, PartialEq)]
struct Args {
    /// `--novr` / `--openxr` / `--openvr`（`--profile` / `--webui` 以外の最初の引数）。
    runtime: Option<String>,
    /// `--profile` で `Some(None)`（既定のダンプ先）、`--profile=PATH` で `Some(Some(PATH))`。
    profile: Option<Option<PathBuf>>,
    /// `--webui`: UI 層（GPUI）の代わりに Web UI 層（Tauri）で画面を出す。
    webui: bool,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut out = Self::default();
        for a in args {
            match a.strip_prefix("--profile") {
                _ if a == "--webui" => out.webui = true,
                Some("") => out.profile = Some(None),
                Some(rest) if rest.starts_with('=') => {
                    out.profile = Some(Some(PathBuf::from(&rest[1..])))
                }
                _ if out.runtime.is_none() => out.runtime = Some(a),
                _ => logger::warn!("知らない引数を無視する", arg = a),
            }
        }
        out
    }
}

/// プロファイラを起こす。ダンプ先は既定 `os::log_dir()/profile.csv`（前回分は `profile-prev.csv`）。
/// 作れなくても計測と表示はする。
fn start_profiler(path: Option<PathBuf>) -> profiler::Profiler {
    let path = path.or_else(|| os::log_dir().map(|d| d.join("profile.csv")));
    let out = path.as_ref().and_then(|p| match os::create_rotated(p) {
        Ok(f) => Some(Box::new(BufWriter::new(f)) as Box<dyn Write>),
        Err(e) => {
            logger::warn!(
                "プロファイルのダンプ先を作れない",
                path = p.display(),
                error = e
            );
            None
        }
    });
    if let (Some(p), Some(_)) = (&path, &out) {
        logger::info!("プロファイルを CSV にダンプする", path = p.display());
    }
    profiler::Profiler::new(out).unwrap_or_else(|e| {
        logger::warn!("プロファイルの CSV を書けないので表示だけにする", error = e);
        profiler::Profiler::new(None).expect("出力先が無ければ失敗しない")
    })
}

/// 標準エラー出力と `os::log_dir()/nyaw.log` へ出すロガーを設定する。レベルは `NYAW_LOG` > 設定ファイルの `log`（既定 info）。
fn init_logger(from_config: Option<String>) {
    let requested = os::log_level().or(from_config);
    let level = requested.as_deref().and_then(|s| s.parse().ok());
    let path = os::log_dir().map(|d| d.join("nyaw.log"));
    let file = path.as_ref().map(os::FileSink::create);

    let mut logger = Logger::new(level.unwrap_or(Level::Info)).sink(StderrSink);
    let mut file_error = None;
    match file {
        Some(Ok(sink)) => logger = logger.sink(sink),
        Some(Err(e)) => file_error = Some(e),
        None => {}
    }
    logger::init(logger).expect("ロガーの初期化は main の最初に 1 回だけ");

    // パニックもロガーを通して残す（ファイルにも書かれる）
    std::panic::set_hook(Box::new(|info| {
        logger::error!("パニック", info = info);
        logger::global().flush();
    }));

    if let (Some(raw), None) = (&requested, level) {
        logger::warn!("ログレベルを解釈できないので info にする", value = raw);
    }
    match (path, file_error) {
        (Some(p), None) => logger::debug!("ログをファイルにも出す", path = p.display()),
        (Some(p), Some(e)) => logger::warn!(
            "ログファイルを開けないので標準エラー出力のみに出す",
            path = p.display(),
            error = e
        ),
        (None, _) => logger::warn!("ログファイルの置き場が決まらないので標準エラー出力のみに出す"),
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
                ..Args::default()
            }
        );
        assert_eq!(
            parse(&["--profile", "--openxr"]),
            Args {
                runtime: Some("--openxr".into()),
                profile: Some(None),
                ..Args::default()
            }
        );
        assert_eq!(
            parse(&["--openvr", "--profile=out/p.csv"]),
            Args {
                runtime: Some("--openvr".into()),
                profile: Some(Some(PathBuf::from("out/p.csv"))),
                ..Args::default()
            }
        );
    }

    #[test]
    fn webui_flag_selects_web_frontend_with_any_runtime() {
        assert_eq!(
            parse(&["--webui", "--novr"]),
            Args {
                runtime: Some("--novr".into()),
                webui: true,
                ..Args::default()
            }
        );
        assert!(!parse(&["--openxr"]).webui);
    }
}
