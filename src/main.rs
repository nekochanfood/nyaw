//! ブートローダー: ロガーを組み立て、ランタイム（OpenVR / OpenXR / novr）を選び、メインスレッドと載せる画面（Home）を UI 層に引き渡す。それ以外は何も置かない。

use std::process::ExitCode;

use logger::{Level, Logger, StderrSink};
use vr::Runtime;

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
    // 優先順位: コマンドライン > 設定ファイル
    let prefer = match std::env::args().nth(1).as_deref().or(config.runtime.as_deref()) {
        Some("--novr" | "novr") => Some(Runtime::NoVr),
        Some("--openxr" | "openxr") => Some(Runtime::OpenXr),
        Some("--openvr" | "openvr") => Some(Runtime::OpenVr),
        _ => None,
    };
    let backend = match vr::connect(prefer) {
        Ok(b) => b,
        // 指定なしで VR ランタイムが無ければ novr で起動する（novr は常に使える）
        Err(e) if prefer.is_none() => {
            logger::warn!("VR ランタイムに接続できないので novr で起動する", error = e);
            vr::connect(Some(Runtime::NoVr)).expect("novr は常に接続できる")
        }
        Err(e) => {
            logger::error!("VR ランタイムに接続できない", runtime = ?prefer, error = e);
            return ExitCode::FAILURE;
        }
    };
    logger::info!("ランタイムを選択", runtime = ?backend.runtime());
    ui::run(backend, |_, _| home::Home::default());
    ExitCode::SUCCESS
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
