//! UI を経由する操作シナリオ群。ファイルごとに個別バイナリを生成するとリンクコストが本数分発生するため、1 バイナリにモジュールとしてまとめている。
//! 単一ファイルのみ実行: `cargo test -p testkit --test scenarios -- dialog::`
//!
//! 実時間に依存する判定（長押し・効果音の終了待ち）を含むテスト（`tests/audio.rs`・`tests/pointer_mode.rs`）はここに含めない。
//! 同一バイナリに含めると全コアで並列実行されて CPU 競合が発生し、タイムアウトで判定が不安定になる。

mod attach;
mod dashboard;
mod desktop_stream;
mod dialog;
mod emoji;
mod hold_in_hand;
mod keyboard;
mod multi_panel;
mod nav_bar;
mod navigation;
mod novr_emulator;
mod panel_texture;
mod pointer_settings;
mod prefs;
mod setup;
mod setup_lockdown;
mod skeleton;
mod test_panels;
mod testing;
mod testing_transparent;
mod tooltip;
mod update;
mod vr_controls;
mod wizard;
