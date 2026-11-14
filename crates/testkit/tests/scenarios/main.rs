//! UI を通す操作シナリオ。ファイルごとに別バイナリにするとリンクが本数分かかるので、1 本にまとめてモジュールにしている。
//! 1 ファイルだけ流す: `cargo test -p testkit --test scenarios -- dialog::`
//!
//! 実時間で判定する操作（長押し・効果音の鳴り終わり）を見るテスト（`tests/audio.rs`・`tests/pointer_mode.rs`）はここに入れない。
//! まとめると全コアで同時に走って CPU が混み、判定が時間切れで外れる

mod attach;
mod dashboard;
mod desktop_stream;
mod dialog;
mod emoji;
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
