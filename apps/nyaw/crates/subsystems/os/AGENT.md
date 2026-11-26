# crates/subsystems/os

層: 1 Subsystems

データ保存先ディレクトリ、VR ランタイム検出、高精度タイマー設定、GPU プロセス優先度制御など、オペレーティングシステム固有の機能差を吸収する。

OS 固有の条件付きコンパイル（`#[cfg(windows)]` 等）は、その処理を担当する各サブシステムの中に閉じる（たとえば画面キャプチャは `desktop`、描画ホスト固有の処理は `rendering`）。各サブシステムは OS ごとの実装を `src/windows.rs` のような別ファイルに置く。本クレートが受け持つのは、保存先パスやタイマーのように特定のサブシステムに属さない OS 差異である。

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
data_search_dirs();         // 共有データの検索先の取得（Linux のみ）
precise_timers();           // 高精度タイマーの有効化
high_gpu_priority();        // GPU プロセス優先度の引き上げ
ui_language();              // OS 表示言語の取得
openvr_paths_file();        // OpenVR 設定ファイルのパス取得
openxr_runtime_manifest();  // OpenXR アクティブランタイムマニフェストのパス取得
attach_console();           // 親コンソールへのアタッチ（--console 指定時）
error_dialog(title, msg);   // 致命的エラー時のネイティブダイアログ表示
```

## OS ごとの実装

バックエンドは Windows（`src/windows.rs`）と Linux（`src/linux.rs`）の 2 つである。それ以外の OS ではコンパイルエラーとする。Linux の保存先は XDG Base Directory に従う。

| 項目 | Windows | Linux |
|---|---|---|
| 設定（`config_dir`） | `%LOCALAPPDATA%\nyaw` | `$XDG_CONFIG_HOME/nyaw`（既定 `~/.config/nyaw`） |
| データ（`data_dir`） | `%LOCALAPPDATA%\nyaw` | `$XDG_DATA_HOME/nyaw`（既定 `~/.local/share/nyaw`） |
| ログ（`log_dir`） | `%LOCALAPPDATA%\nyaw\logs` | `$XDG_STATE_HOME/nyaw/logs`（既定 `~/.local/state/nyaw/logs`） |
| 共有データの検索先（`data_search_dirs`） | なし | `$XDG_DATA_HOME`、`$XDG_DATA_DIRS`（既定 `/usr/local/share:/usr/share`） |
| OpenVR のパス定義 | `%LOCALAPPDATA%\openvr\openvrpaths.vrpath` | `$XDG_CONFIG_HOME/openvr/openvrpaths.vrpath` |
| OpenXR のランタイム | `XR_RUNTIME_JSON`、レジストリの `ActiveRuntime` | `XR_RUNTIME_JSON`、`openxr/1/active_runtime.json`（OpenXR ローダーと同じ探索順） |
| 表示言語 | `GetUserDefaultUILanguage` | 環境変数 `LC_ALL` > `LC_MESSAGES` > `LANG` |
| タイマーの精度 | 1ms に引き上げる | 変更しない（`sleep` は hrtimer により高精度） |
| GPU の優先度 | HIGH に引き上げる | 汎用の API がないため引き上げない（常に `false`） |
| エラーのダイアログ | `MessageBoxW` | zenity または kdialog。どちらもなければログに記録する |

キーボード（`keyboard`）の Linux バックエンドは、キー配列の検出のみを行う。配列は `XKB_DEFAULT_LAYOUT` > `setxkbmap -query` > `localectl status` の順に探索し、起動時に一度だけ検出する。OS への入力送信（Windows の `SendInput` に相当）には表示サーバーとの接続が要るため、Linux では `desktop` が `Host` を実装して送信を担い、配列は `keyboard::detect` から受け取る（[`../desktop/AGENT.md`](../desktop/AGENT.md)）。このバックエンド単体の送信は、常にエラーを返す。
