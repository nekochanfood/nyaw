//! ブートローダー: VR ランタイムを選び、メインスレッドを UI 層に引き渡す。それ以外は何も置かない。

use std::process::ExitCode;

fn main() -> ExitCode {
    let prefer = match std::env::args().nth(1).as_deref() {
        Some("--openxr") => Some(vr::Runtime::OpenXr),
        Some("--openvr") => Some(vr::Runtime::OpenVr),
        _ => None,
    };
    match vr::connect(prefer) {
        Ok(backend) => {
            ui::run(backend);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("nyaw: {e}");
            ExitCode::FAILURE
        }
    }
}
