# crates/subsystems/os

層: 1 Subsystems

保存先、ランタイム検出、タイマー精度・GPU 優先度を提供。現在は Windows バックエンドを実装。OS の一般的な `#[cfg]` はここに書く（描画固有は `rendering`）

## 主な公開 API

`config_dir`, `log_dir`, `data_dir`（ニャプリのパッケージと保存領域）, `precise_timers`, `high_gpu_priority`, `ui_language`（OS の表示言語。`ja` / `en` 以外は `None`）, `openvr_paths_file`, `openxr_runtime_manifest`
