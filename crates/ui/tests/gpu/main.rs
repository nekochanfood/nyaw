//! 実プラットフォーム + GPU で動かすテスト。GUI はメインスレッドで回し、GPUI のアプリは 1 プロセスで 1 回しか起こせないので、
//! 各ケースは独自の main を持つ。ファイルごとに別バイナリにするとリンクが本数分かかるので 1 本にまとめ、
//! このバイナリが自分自身をケースごとに子プロセスとして起こす（`NYAW_GPU_CASE` でどのケースか渡す）。
//!
//! 1 ケースだけ流す: `cargo test -p ui --test gpu -- --ignored stress`（名前の一部で絞る）

mod common;
mod emoji;
mod font_weights;
mod gpu_capture;
mod keyboard;
mod look;
mod nav_swipe;
mod novr_cursor;
mod novr_keys;
mod panel_lod;
mod round_clip;
mod stress;
mod superellipse;
mod test_panels;

use std::process::{Command, ExitCode, Termination};

const CASE: &str = "NYAW_GPU_CASE";

/// ケースの main（`()` か `ExitCode` を返す）を終了コードにする。
fn report<T: Termination>(main: fn() -> T) -> ExitCode {
    main().report()
}

type Case = (&'static str, fn() -> ExitCode);

const CASES: &[Case] = &[
    ("emoji", || report(emoji::main)),
    ("font_weights", || report(font_weights::main)),
    ("gpu_capture", || report(gpu_capture::main)),
    ("keyboard", || report(keyboard::main)),
    ("look", || report(look::main)),
    ("nav_swipe", || report(nav_swipe::main)),
    ("novr_cursor", || report(novr_cursor::main)),
    ("novr_keys", || report(novr_keys::main)),
    ("panel_lod", || report(panel_lod::main)),
    ("round_clip", || report(round_clip::main)),
    ("stress", || report(stress::main)),
    ("superellipse", || report(superellipse::main)),
    ("test_panels", || report(test_panels::main)),
];

fn main() -> ExitCode {
    if let Ok(name) = std::env::var(CASE) {
        let (_, run) = CASES
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("unknown case {name}"));
        return run();
    }

    // ponytail: `-` で始まらない引数はすべて名前の絞り込みとみなす。`--test-threads 1` のように値を分けて渡すと値も絞り込みになるので `=` で渡す
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let exe = std::env::current_exe().expect("cannot find the test binary");
    let mut failed = Vec::new();
    for (name, _) in CASES
        .iter()
        .filter(|(n, _)| filters.is_empty() || filters.iter().any(|f| n.contains(f.as_str())))
    {
        let status = Command::new(&exe)
            .args(std::env::args_os().skip(1))
            .env(CASE, name)
            .status()
            .expect("cannot start the test case");
        if !status.success() {
            failed.push(*name);
        }
    }
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        println!("gpu: FAILED {failed:?}");
        ExitCode::FAILURE
    }
}
