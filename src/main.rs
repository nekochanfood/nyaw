//! ブートローダー: ランタイム（OpenVR / OpenXR / novr）を選び、メインスレッドを UI 層に引き渡す。それ以外は何も置かない。

use std::process::ExitCode;

use vr::Runtime;

fn main() -> ExitCode {
    let prefer = match std::env::args().nth(1).as_deref() {
        Some("--novr") => Some(Runtime::NoVr),
        Some("--openxr") => Some(Runtime::OpenXr),
        Some("--openvr") => Some(Runtime::OpenVr),
        _ => None,
    };
    let backend = match vr::connect(prefer) {
        Ok(b) => b,
        // 指定なしで VR ランタイムが無ければ novr で起動する（novr は常に使える）
        Err(e) if prefer.is_none() => {
            eprintln!("nyaw: {e}");
            vr::connect(Some(Runtime::NoVr)).expect("novr は常に接続できる")
        }
        Err(e) => {
            eprintln!("nyaw: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("nyaw: runtime = {:?}", backend.runtime());
    ui::run(backend);
    ExitCode::SUCCESS
}
