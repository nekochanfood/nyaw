//! 実プラットフォーム + GPU で動作するテスト。GUI はメインスレッドで実行し、GPUI のアプリは 1 プロセスで 1 回しか起動できないため、
//! 各ケースは独自の main を持つ。ファイルごとに別バイナリにするとリンクが本数分必要になるため 1 本にまとめ、
//! このバイナリが自分自身をケースごとに子プロセスとして起動する（`NYAW_GPU_CASE` でケースを指定する）。
//!
//! 1 ケースだけ流す: `cargo test -p ui --test gpu -- --ignored stress`（名前の一部で絞る）
// ケースを子プロセスとして起動し、描画結果を画像ファイルに書き出すため、プロセスとファイル操作の制限（clippy.toml）を外す。
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

mod a11y;
mod common;
mod dock_magnet;
mod emoji;
mod font_weights;
mod gpu_capture;
mod guide;
mod intro_logo;
mod keyboard;
mod look;
mod look_toast;
mod nav_swipe;
mod novr_cursor;
mod novr_keys;
mod panel_lod;
mod rotation;
mod round_clip;
mod site;
mod stress;
mod stroke_caps;
mod superellipse;
mod test_panels;
mod tutorial;

use std::process::{Command, ExitCode, Termination};

const CASE: &str = "NYAW_GPU_CASE";

/// ケースの main（`()` または `ExitCode` を返す）を終了コードに変換する。
fn report<T: Termination>(main: fn() -> T) -> ExitCode {
    main().report()
}

type Case = (&'static str, fn() -> ExitCode);

const CASES: &[Case] = &[
    ("a11y", || report(a11y::main)),
    ("dock_magnet", || report(dock_magnet::main)),
    ("emoji", || report(emoji::main)),
    ("font_weights", || report(font_weights::main)),
    ("gpu_capture", || report(gpu_capture::main)),
    ("guide", || report(guide::main)),
    ("intro_logo", || report(intro_logo::main)),
    ("keyboard", || report(keyboard::main)),
    ("look", || report(look::main)),
    ("look_toast", || report(look_toast::main)),
    ("nav_swipe", || report(nav_swipe::main)),
    ("novr_cursor", || report(novr_cursor::main)),
    ("novr_keys", || report(novr_keys::main)),
    ("panel_lod", || report(panel_lod::main)),
    ("rotation", || report(rotation::main)),
    ("round_clip", || report(round_clip::main)),
    ("site", || report(site::main)),
    ("stress", || report(stress::main)),
    ("stroke_caps", || report(stroke_caps::main)),
    ("superellipse", || report(superellipse::main)),
    ("test_panels", || report(test_panels::main)),
    ("tutorial", || report(tutorial::main)),
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
