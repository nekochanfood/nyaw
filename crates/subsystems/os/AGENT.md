# crates/subsystems/os

層: 1 Subsystems

データ保存先ディレクトリ、VR ランタイム検出、高精度タイマー設定、GPU プロセス優先度制御など、オペレーティングシステム固有の機能差を吸収する。

OS 固有の条件付きコンパイル（`#[cfg(windows)]` 等）は原則として本クレート内に集約する（描画ホスト固有の処理は `crates/subsystems/rendering` に集約）。

## 主な責務と仕様

- **パス管理**: アプリケーション設定（`config_dir`）、ログファイル（`log_dir`）、およびユーザーデータ（`data_dir`）の配置先パスを提供する。開発ビルド（debug）ではディレクトリ名を `nyaw-dev` に分離し、リリース版の設定との衝突を防止する。
- **高精度タイマー (`precise_timers`)**: Windows 環境においてスリープ精度の低下を防ぐため、OS のタイマー分解能を 1ms に引き上げる。
- **システム言語検出 (`ui_language`)**: OS の表示言語を取得し、日本語・英語の対応コード（`"ja"` または `"en"`）を返す（未対応言語の場合は `None`）。
- **ランタイム検出**: システムにインストールされている OpenVR パス定義ファイルや OpenXR ランタイムマニフェストの位置を探索する。

## 主な公開 API

```rust
config_dir();               // 設定保存先ディレクトリの取得
log_dir();                  // ログ保存先ディレクトリの取得
data_dir();                 // ユーザーデータ保存先ディレクトリの取得
precise_timers();           // 高精度タイマーの有効化
high_gpu_priority();        // GPU プロセス優先度の引き上げ
ui_language();              // OS 表示言語の取得
openvr_paths_file();        // OpenVR 設定ファイルのパス取得
openxr_runtime_manifest();  // OpenXR アクティブランタイムマニフェストのパス取得
attach_console();           // 親コンソールへのアタッチ（--console 指定時）
error_dialog(title, msg);   // 致命的エラー時のネイティブダイアログ表示
```
