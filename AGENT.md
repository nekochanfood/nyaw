# AGENT.md

nyaw は、GPUI で GUI を描画する VR オーバーレイシステムである。Cargo ワークスペースを採用しており、関心事に応じて層を定義し、原則として 1 つの責務に対して 1 クレートを割り当てる。

## アーキテクチャの階層構造

システムは下位から上位へ向かう 8 つの層で構成される。すべてのクレートはいずれかの層に属する。新しいクレートを追加する場合も、所属する層を決定したうえで本目次へ追加する。

| 層 | 名前 | 位置づけ |
|---|---|---|
| 0 | 基盤 | どの層からも参照できる共有クレート。他クレートへの実行時コード依存を持たない。 |
| 1 | Subsystems | OS、VR ランタイム、入力機器、描画環境などの差異を吸収する。 |
| 2 | オーバーレイ | 3 次元空間内におけるパネルの配置、姿勢計算、当たり判定を処理する。 |
| 3 | カーネル | アプリケーション設定、フレーム進行、パネル操作を統括する。 |
| 4 | UI | GPUI の非表示ウィンドウを用いたパネル描画と、入力イベントの変換を行う。 |
| 5 | 画面 | UI 層の上に構築される個別の機能画面（ダッシュボードや設定画面など）。 |
| 6 | ブート | エントリポイント（`src/main.rs`）。設定読み込みとランタイム初期化を行い、イベントループを開始する。 |
| 7 | テスト | 開発専用のエンドツーエンド（E2E）テスト基盤。本番バイナリには含まれない。 |

### クレート一覧

各クレートの詳細な責務や公開 API については、それぞれのリンク先にある `AGENT.md` を参照する。

| 層 | クレート | 概要 |
|---|---|---|
| 0 | [`crates/logger`](crates/logger/AGENT.md) | logfmt 形式に対応した横断的構造化ロガー。 |
| 0 | [`crates/animation`](crates/animation/AGENT.md) | アニメーション計算と遷移状態の管理。GPUI や UI 部品には依存しない。 |
| 1 | [`crates/subsystems`](crates/subsystems/AGENT.md) | 各サブシステムの再エクスポートと、VR ランタイムの入力から共通フレーム情報を作る処理。 |
| 1 | [`crates/subsystems/os`](crates/subsystems/os/AGENT.md) | データ保存先パス、ランタイム検出、高精度タイマーなどの OS 差異の吸収。 |
| 1 | [`crates/subsystems/vr`](crates/subsystems/vr/AGENT.md) | OpenVR、OpenXR、および非 VR 用エミュレーター（novr）のランタイム抽象化。 |
| 1 | [`crates/subsystems/input`](crates/subsystems/input/AGENT.md) | コントローラの機種別入力マッピングと骨格ハンドトラッキングの処理。 |
| 1 | [`crates/subsystems/rendering`](crates/subsystems/rendering/AGENT.md) | GPUI のオフスクリーン描画、ピクセル読み戻し、VR ランタイムへの転送。 |
| 1 | [`crates/subsystems/audio`](crates/subsystems/audio/AGENT.md) | 音声出力および Steam Audio による空間音響処理。 |
| 1 | [`crates/subsystems/keyboard`](crates/subsystems/keyboard/AGENT.md) | 仮想キーボードのレイアウト定義、文字入力、修飾キー状態の管理。 |
| 1 | [`crates/subsystems/desktop`](crates/subsystems/desktop/AGENT.md) | 画面・ウィンドウのキャプチャ、画像取得、OS 側への入力注入。 |
| 1 | [`crates/subsystems/media`](crates/subsystems/media/AGENT.md) | システム内で再生中のメディア情報の取得と再生制御。 |
| 1 | [`crates/subsystems/notifications`](crates/subsystems/notifications/AGENT.md) | OS 通知の取得と、通知クリック時の対象アプリ起動連携。 |
| 1 | [`crates/subsystems/update`](crates/subsystems/update/AGENT.md) | リリース一覧の取得、インストーラーのダウンロードと検証、インストーラーの起動。 |
| 2 | [`crates/overlay`](crates/overlay/AGENT.md) | パネル配置、アタッチ処理、光線交差判定、掴み移動、親子関係、暗転演出の制御。 |
| 3 | [`crates/kernel`](crates/kernel/AGENT.md) | 設定管理、毎フレームの更新サイクル、パネル操作を統合する中核。 |
| 4 | [`crates/ui`](crates/ui/AGENT.md) | GPUI のオフスクリーンウィンドウによるパネル描画とポインタイベント変換。 |
| 4 | [`crates/design`](crates/design/AGENT.md) | デザインシステム（トークン、共通コンポーネント、テーマ管理）。 |
| 4 | [`crates/profiler`](crates/profiler/AGENT.md) | 計測時のみ動作する軽量プロファイラ。 |
| 4 | [`crates/i18n`](crates/i18n/AGENT.md) | 画面表示文字列の多言語対応（英語・日本語）。 |
| 5 | [`crates/testing`](crates/testing/AGENT.md) | 開発時の動作確認・E2E テスト用の各種検証画面。 |
| 5 | [`crates/wrist`](crates/wrist/AGENT.md) | 手首に常駐する腕時計型パネル。 |
| 5 | [`crates/toast`](crates/toast/AGENT.md) | OS 通知のトースト表示（HUD、手首、ダッシュボード間を移動）。 |
| 5 | [`crates/dashboard`](crates/dashboard/AGENT.md) | ランチャー、画面配信、再生制御、クイック設定を統合したダッシュボード。 |
| 5 | [`crates/settings`](crates/settings/AGENT.md) | カテゴリ別に整理されたアプリケーション詳細設定画面。 |
| 5 | [`crates/setup`](crates/setup/AGENT.md) | 初回起動時や設定リセット時に表示するセットアップウィザード。 |
| 5 | [`crates/beam`](crates/beam/AGENT.md) | コントローラのレーザー光線、カーソル表示、ドラッグ挙動、ツールチップ。 |
| 5 | [`crates/updater`](crates/updater/AGENT.md) | 更新の状態と、更新確認・インストールの操作（ダッシュボードと設定画面が共有する。画面ではない）。 |
| 6 | `src/main.rs` | アプリケーション起動処理。ロガー初期化、設定読み込み、初期パネル生成を担う。 |
| 7 | [`crates/testkit`](crates/testkit/AGENT.md) | 宣言的な E2E シナリオテスト用テストハーネス（開発専用）。 |

## ドキュメントの配置方針

本リポジトリでは、情報の重複と散逸を防ぐためにドキュメントの記載範囲を厳密に分離する。

- **ルート `AGENT.md` の役割**: システム全体の目次、アーキテクチャ境界、層間の依存関係、共通の設計思想および規約のみを記載する。特定のクレート単体に閉じた話題はここに含めない。
- **クレート別 `AGENT.md` の役割**: 各クレートの責務、公開 API、内部設計、テスト手順は `crates/<クレート名>/AGENT.md`（サブシステムは `crates/subsystems/<名前>/AGENT.md`）に記載する。他のクレートの仕様を複製せず、`../kernel/AGENT.md` のように相対リンクで参照する。
- **構成の統一**: 各クレートの `AGENT.md` は、冒頭に「層」と「責務」を明記し、続いて「主な公開 API」、その後に設計思想や固有の仕様を記述する構成とする。

## 依存関係とフィーチャー設計

モジュール間の疎結合性を維持し、ビルド時間とテスト実行速度を最適化するために、依存の方向とフィーチャーの切り替えを厳格に管理する。

### 依存の方向性

依存関係は原則として上位層から下位層への単方向とする。逆方向の依存や循環依存は禁止する。

```
subsystems ← overlay ← kernel ← ui ← 画面 (dashboard / wrist など) ← main
```

- **カーネルとサブシステムの関係**: カーネルは設定およびフレーム進行の目的で `subsystems` を参照する。Subsystems 内部の依存順序は `os ← vr ← input / rendering`、`os ← keyboard`、`desktop ← media / notifications`（アプリ情報の解決と起動を `desktop` にまとめるため）とし、上位層への逆参照は行わない。`audio` は他サブシステムに依存せず、位置計算には `glam` を用いる。
- **OS 機能へのアクセス**: OS の API（プロセス・ファイル・通信、`windows` クレートなど）を直接呼べるのは、サブシステム・カーネル・`logger`・`testkit` だけである。上位層（UI や画面など）が OS の機能を使うときは、サブシステムの関数（`kernel` の再エクスポート経由）か、カーネルへの依頼（パネルへの操作は `PanelCommand`、システムへの要求は `Request`）を通す。必要な関数がなければ、先にサブシステムへ追加する。この規則は `clippy.toml` の `disallowed-methods` / `disallowed-types` が検査する（後述の「OS 機能の禁止の仕組み」）。
- **型の共通語彙**: UI 層、画面、`testkit` が参照する下位の型は、`kernel` の再エクスポート経由で取得する。`overlay` は `vr` の型（`Pose` など）を共通語彙として直接利用してよい。
- **横断クレートの例外**: `logger` はすべての層から直接参照できる唯一の横断クレートである。ただし、`logger` 自体はワークスペース内の自前クレートに一切依存しない。
- **デザインシステムと動きの分離**: `design` の定数定義（Constants）および `animation` クレートは GPUI に依存しないため、`overlay` がパネルのアニメーションや余白計算に参照できる。GPUI に依存するコンポーネントやテーマ定義は UI 層以上のみで利用する。

### OS 機能の禁止の仕組み

ルートの `clippy.toml` は、標準ライブラリのプロセス・ファイル・通信の API（`std::process::Command`、`std::fs::*`、`std::net::*`）と、`ureq`・gpui の OS 機能（`App::open_url` など）の使用を禁止する。CI の `cargo clippy --workspace --all-targets -- -D warnings` が違反を検出する。上位層は `windows` クレートに依存していないため、その API を呼ぶコードはコンパイルできない。

- **許可する側の設定**: clippy は設定ファイルを継承・結合せず、クレートのディレクトリから親へ向かって最初に見つけた 1 つだけを使う。そのため `crates/kernel`・`crates/logger`・`crates/subsystems`・`crates/testkit` は、OS の機能以外の規則（URL を開く処理の制限）だけを複製した `clippy.toml` を持つ。ルートの規則を変えたら複製も揃える。
- **ビルドスクリプトとテスト**: 上位層のビルドスクリプト（コード生成）と、ファイルやプロセスを使うテスト（`ui/tests/gpu` の画像出力など）は、ファイルや関数の先頭に `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]` と理由のコメントを置く。アプリ本体のコードには置かない。

### フィーチャーによる依存の制御

VR ランタイムや重い依存関係（wgpu、minifb、OpenVR、OpenXR）は Cargo フィーチャーによって局所化する。

- `vr` のバックエンド切り替えフィーチャー（`novr` / `openvr` / `openxr`）は、`subsystems` および `kernel` を通じて同名で転送される。
- ワークスペース設定において、`vr`、`subsystems`、`kernel` の依存は `default-features = false` を基本とする。`kernel` の既定は最小限（基本型、モック、接続インターフェースのみ）とし、`overlay` や `kernel` 単体のテストでは重いランタイムをリンクしない。
- 実際のアプリケーションバイナリ（`nyaw`）や、実描画・エミュレーターを必要とする `ui` および `testkit` のテストでのみ必要なフィーチャーを有効化する。無効化されたランタイムへの接続要求に対しては `VrError::NoRuntime` を返す。

## ログ設計

ログの整合性と可読性を保つため、出力形式と出力先を統一する。

- **マクロの使用**: ログ出力には必ず `logger` クレートが提供する構造化マクロ（`logger::info!` / `warn!` / `error!`）を使用する。`println!` や `eprintln!` の直接使用は禁止する（テストハーネス内の出力は除く）。
  ```rust
  logger::warn!("パネル描画の取得に失敗しました", error = e, panel = ?panel);
  ```
- **出力フォーマット**: 1 行 1 件の logfmt 形式を採用する。先頭は `ts=... level=... target=... msg=...` で固定し、以降に任意のキー・値フィールドを記述順で続ける。キー名は snake_case とし、`target` には `module_path!()` を記録する。
- **出力先とログ退避**: 出力先は標準エラー出力（`logger::StderrSink`）とローカルログファイル（`logger::FileSink`、Windows 環境では `%LOCALAPPDATA%\nyaw\logs\nyaw.log`）の 2 系統とする。起動時に前回のログを `nyaw-prev.log` に 1 世代のみ退避し、新規にファイルを作成する。
- **ログレベルの制御**: 環境変数 `NYAW_LOG`（`trace` / `debug` / `info` / `warn` / `error`、既定値は `info`）で動的に制御する。初期化完了前のログは標準エラー出力へ `info` レベル以上で出力される。外部ライブラリ（GPUI など）のログも同ロガーへ転送し、パニック時もフックを通じて `error` として記録する。

## ランタイムの設計思想

nyaw において、novr（ウィンドウ型エミュレーター）、SteamVR (OpenVR)、OpenXR はすべて対等な入出力手段として扱う。

- **novr の位置づけ**: novr はテスト専用の暫定実装ではなく、実機 VR を持たない環境や通常開発において日常的に利用される正式なランタイムである。そのため、ビルド済みバイナリにおいても常時利用可能とする。
- **上位層の非依存性**: UI 層以上のコンポーネントは、背後で動作しているランタイムの種類によって処理を分岐させない。どのランタイムであっても、パネルは GPUI の非表示オフスクリーンウィンドウ上で描画され、ピクセルデータとして読み戻された後にオーバーレイ層へ転送される。
- **機能差の許容**: コントローラの 6 自由度トラッキングやパネルの直接掴み移動など、VR ハードウェアに固有の機能が novr 上で代替されないことは仕様として許容する。ただし、ポインタによる基本操作や画面遷移はすべてのランタイムで同一に実行できなければならない。

## フレームサイクルの処理手順

メインスレッドにおける 1 フレームの進行（`ui::run`）は、以下のパイプラインに沿って実行される。周期生成は `poll` が担い、VR 環境ではランタイムの垂直同期信号（vsync）、novr 環境では約 90Hz の周期で駆動する。

```
ui::run
 └ ui::pump → kernel.tick()
     ├ subsystems.poll()       → 共通 Frame 生成（HMD姿勢、コントローラ状態、終了要求）
     │   ├ backend.poll()      → ランタイム生入力の取得（Subsystems 内部で完結）
     │   └ input::map          → 機種別変換、ヒステリシス計算（接続変更時に履歴破棄）
     ├ overlay.tick_frame      → 共通フレーム情報をもとにパネルの姿勢・操作を計算
     ├ パネル姿勢更新          → 追従パネルの座標計算（掴み中は手に追従、解放時にオフセット再計算）
     ├ 光線交差判定            → レーザーと各パネルの当たり判定 → Vec<UiEvent> を生成
     ├ ポインタ表示更新        → レーザー長を衝突面まで調整、カーソル座標を設定
     └ vr.set_overlay_pose     → 計算結果の姿勢をランタイムへ反映
 └ PointerBridge::translate    → UiEvent を gpui::PlatformInput に変換し各ウィンドウへ配送
 └ ui::sync_panel              → PanelControl に積まれた操作（アタッチ、ロック等）をカーネルへ反映
 └ PanelControl::spawns        → 新規要求されたパネルのウィンドウを生成
 └ PanelControl::quit          → アプリケーション終了要求の処理
 └ ui::capture(window)         → オフスクリーンウィンドウを描画し RGBA ピクセルを取得
 └ overlay.submit(vr, ...)     → 描画結果を VR ランタイムのオーバーレイへ転送
```

### ウィンドウ管理の注意点

パネル用ウィンドウは `ui::open_panel_window` を用いてオフスクリーン（非表示）として生成する。描画結果の取得には `Window::render_to_image` を使用する。

ルートビューを借用した状態で描画や入力配送を行うと GPUI 内部で多重借用パニックが発生するため、ウィンドウに対する操作は `WindowHandle::update` ではなく、必ず `cx.update_window(handle.into(), ...)` を介して実行する。また、GPU の同期負荷を抑えるため、ピクセル読み戻しは画面内容に変更があった場合、または最低 250ms 間隔に限定して実行する。

## 画面の分離と制御

UI 層（`crates/ui`）は単一の共通基盤であり、特定の画面の個別仕様を知らない。各画面は独立したクレート（`crates/dashboard`、`crates/wrist` など）として実装され、実行時に `PanelSpec` の集合として渡される。

画面どうしは原則として依存しない。複数の画面が使う部品は UI 層に置く（トーストとダッシュボードが使う `ui::notification` など）。例外は次の 2 つである。

- **シェルとしての `wrist`**: `wrist` は腕時計の表示に加えて、シェルの役割（ダッシュボードの開閉、トーストの生成、ロックダウンへの対応）を兼ねる。そのため `dashboard` と `toast` に依存する。
- **開発用の `testing`**: 設定画面の部品を検証するため `settings` に依存する。

### 画面とカーネル間の通信

画面からカーネルに対する状態変更の要求は、GPUI のグローバルリソースである `PanelControl` を介して非同期に行う。

1. **個別パネルの操作**: パネル固有の操作（取り付け、取り外し、ロック等）は、PanelId を指定して `PanelControl` に `PanelCommand` を積む。フレームループ内の `sync_panel` がこれをカーネルへ適用し、最新の `PanelStatus` を画面側へ書き戻す。
2. **全体設定の変更**: テーマや言語などのシステム全体に関わる設定は、`PanelControl::setting_changes` に `Setting` を積む。`sync_settings` がこれをカーネルへ反映し、`PanelControl::settings` を更新する。
3. **システムへの要求**: メディアの操作や OS の通知の操作など、どのパネルにも属さない要求は、`PanelControl::requests` に `Request` を積む。`sync_settings` がこれをカーネルへ渡す（`Kernel::request`）。
4. **ヘルパー関数の利用**: 画面側からコマンドを発行する際は、重複コードの作成を避け、`ui::command`・`ui::set`・`ui::request` などの共通入口関数を使用する。

## パフォーマンス設計原則

オーバーレイはユーザーの視界内に常時表示されるため、フレーム落ち（スタッター）は重大な操作感の低下を招く。メインスレッドにおける処理時間は厳密に管理されなければならない。

- **メインスレッドの非同期化**: ファイル入出力、画像のデコード、フォント読み込み、重い計算処理をメインスレッドのフレームループ内で直接実行することを禁止する。これらはバックグラウンドスレッドで実行し、結果をチャンネル経由で受け取る。処理がフレームに間に合わない場合は、前フレームの状態を維持して描画を進める。
- **データ並列化の推進**: 画素処理や行単位の走査など、相互に独立した処理はスレッドプールを利用して並列化する。たとえば、novr における画面合成処理は画面を水平帯に分割し、`std::thread::scope` を用いて並列実行することで処理時間を大幅に削減している。
- **タイマー精度の確保**: フレーム周期の待機には、OS による丸め誤差の影響を受けない手段を用いる。Windows 環境の通常スリープはミリ秒単位の精度が出ないため、`os::precise_timers` を通じてタイマー分解能を 1ms に引き上げ、バックグラウンドスレッドで高精度な待機を行う。
- **計測に基づく最適化**: 推測による最適化を避け、ストレステスト（`cargo test -p ui --test gpu -- --ignored stress`）およびプロファイラ（`--profile`）の実測値に基づいて判断する。改修を行う際は、改修前後の実測数値をコミットメッセージに記録する。
- **デバッグビルド時のシェーダーコンパイル抑制**: GPUI の Windows 実装は、debug-assertions が有効な場合にウィンドウ生成ごとに HLSL シェーダーを実行時コンパイルするため、1 枚あたり約 180ms の遅延が発生する。これを防ぐため、`Cargo.toml` の `[profile.dev.package.gpui_windows]` において最適化レベルを指定している。

## 開発規約

### 座標系と語彙の統一

- **語彙定義**: 入力およびプロファイルに関する識別子は OpenXR の仕様に準拠する（例: プロファイル名 `"valve/index_controller"`、入力名 `"trigger/value"`）。OpenVR バックエンド側でこれらの語彙への相互変換を行う。
- **3 次元座標系**: トラッキング空間はメートル単位の右手系（前方 -Z、上 +Y、右 +X）とする。パネル表面はローカル座標系で +Z 方向を正面とし、パネル内の 2 次元ピクセル座標は左上を原点とする。
- **ハードウェア対応の追加**: 新しい VR コントローラへの対応は `crates/subsystems/input` の `PROFILES` 定義に追加することで行う。上位層で機種名を直接判定してはならない。
- **ランタイムの追加**: 新規ランタイムの対応は `crates/subsystems/vr` 内に `VrBackend` の実装を追加し、`connect` 関数へ登録する。

### バージョン管理とリリース手順

本プロジェクトは Calendar Versioning（CalVer（現在 `2026.0.3-dev.0`、形式: `YYYY.MINOR.MICRO[-dev.PATCH]`））を採用しており、`Cargo.toml` の `[workspace.package]` で一括管理する。

- **タグ命名規則**: リリースタグは `v<CalVer>`（安定版）または `v<CalVer>-<チャネル名>.<番号>`（プレリリース版）とする。
- **正式リリースの流れ**: GitHub Actions の `prepare-release.yml` ワークフローにバージョン番号を指定して実行すると、バージョン更新の Pull Request が自動生成される。この PR をマージすると、`release.yml` がインストーラーをビルドし、GitHub Releases に正式版が発行される。発行完了後、`main` ブランチのバージョン番号は自動的に次の版の `-dev.0` へ進む。
- **プレリリースの発行**: ブランチ上で Actions の `release.yml` をチャネル名（alpha や beta など）を指定して手動実行すると、プレリリースが発行される。この際、`Cargo.toml` のバージョン表記は変更しない。
- **更新判定の基準**: アプリケーション本体による新旧判定は、バージョン文字列ではなくビルド番号（コミット番号およびビルドメタデータ）の大小比較によって行う。

### コード規約と UI 規則

- **コメントの記述範囲**: コード内のコメントは原則として 1 行、不可欠な場合でも 3 行以内に収める。詳細な設計理由や背景は該当クレートの `AGENT.md` に記述し、コードからはその参照を示すにとどめる。
- **外部リンクの起動**: ブラウザで URL を開く処理は `design::link::open` を経由しなければならない。`cx.open_url` を直接呼び出すことは `clippy.toml` により禁止されている。実行前に確認ダイアログが表示される。
- **意図的簡略化の明記**: 実装をあえて簡略化している箇所には `// ponytail:` プレフィックスを付けたコメントを配置し、現在の制約と将来の改善案を明記する。
- **ナビゲーションバーの設置基準**: 新規パネルを作成する際は、ナビゲーションバーを付与するか、`PanelSpec::without_nav_bar()` で明示的に除外するかを設計ガイドラインに従って判断する。付与する場合は `.titled("...")` でタイトルを渡し、パネル内部に独自の閉じるボタンや重複した見出しを配置しない。

## テスト手法

テストスイートは単体テスト、UI 検証、E2E シナリオテストに分類される。なお、重い E2E テストはローカル環境で実行し、CI ジョブには含めない。

- **ワークスペース全体のテスト実行**: `cargo test --workspace` を実行する。ビルドには cmake および C++ コンパイラが必要となる。CI 環境では `cargo fmt --check`、`cargo clippy`、`cargo test` が並列ジョブとして実行される。
- **単体テスト**: 各クレート内の `#[cfg(test)]` に記述し、自層の範囲内で完結させる。外部ランタイムとの通信は `vr::mock::MockBackend` を使用する。循環依存を防ぐため、単体テストから `testkit` を参照してはならない。
- **シナリオテスト**: `crates/testkit/tests/` に配置する。GPUI の `.debug_selector(|| "識別名".into())` を指定して要素を探索・操作し、プロファイル名や生の座標計算をテストコードに直接書かない。
  - **全ランタイム共通テスト**: クリックやホバーなどの基本操作は `Driver` インターフェースに対して 1 回記述し、`on_every_runtime!` マクロを用いて novr および VR の両環境で網羅実行する。
  - **novr 専用テスト**: UI のレイアウトや表示ロジックの検証は `NoVrKit` を使用する。エミュレーターをヘッドレスで起動し、マウスやキー入力を注入して `screenshot()` で結果を検証する。
  - **VR 固有機能のテスト**: コントローラ把持、レーザーポインタ、空間配置の検証は `VrKit` を使用して記述する。
