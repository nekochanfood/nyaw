# AGENT.md

nyaw は GPUI で GUI を描く VR オーバーレイシステム。Cargo ワークスペースで、層ごとに 1 クレート。

## 層構成（下 → 上）

| 層 | クレート | 責務 | 主な公開 API |
|---|---|---|---|
| - | `crates/logger` | logfmt 形式の構造化ロガー（横断）。整形・レベル・出力先 `Sink` の抽象と、`log` クレート（gpui 等）からの橋渡し | `trace!/debug!/info!/warn!/error!`, `Logger`, `Sink`, `StderrSink`, `init`, `format` |
| 0 OS | `crates/os` | Windows/Linux の差を吸収（パス、環境変数、ランタイム検出、ファイル入出力）。`#[cfg]` はここだけに書く | `config_dir`, `log_dir`, `log_level`, `FileSink`, `openvr_paths_file`, `openxr_runtime_manifest` |
| 1 ランタイム | `crates/vr` | OpenVR / OpenXR / novr の差を吸収。上位層は `Box<dyn VrBackend>` しか持たない | `connect`, `VrBackend`, `Runtime`, `Surface`, `Pose`, `Frame`, `RawController`, `mock::MockBackend`（`test-support` フィーチャー時のみ） |
| 2 入力 | `crates/input` | コントローラ機種の差を吸収（ドライバ的役割）。プロファイル表で生入力 → 抽象アクション | `map(&RawController) -> ControllerState`, `source(profile, Action)`（逆引き） |
| 3 オーバーレイ | `crates/overlay` | パネル配置、レーザーのヒット判定、ポインタイベント生成、パネルの掴み移動 | `Overlay::{add_panel, tick, submit}`, `Panel::pixel_to_world`, `UiEvent`, `PointerEvent` |
| 4 UI | `crates/ui` | GPUI フロントエンド。ポインタイベントを GPUI のマウス入力に変換してビューへ流し、描画結果を読み戻してパネルへ返す | `run`, `pump`（入力1フレーム分。本番と testkit 共通）, `capture`（描画の読み戻し）, `open_panel_window`, `PointerBridge`, ビュー（`Home`） |
| - | `crates/testkit` | Playwright 風 E2E テスト基盤。開発専用。ランタイムごとに対等なキット | `Driver::{click, hover, read, color_at}`（全キット共通）, `on_every_runtime!`, `NoVrKit`, `VrKit::{press, drag_panel, hand, profile, point_at}` |
| - | `src/main.rs` | ブートローダー。ロガーを組み立て、ランタイムを選んで `ui::run` に引き渡すだけ。`--novr` / `--openxr` / `--openvr`、指定なしで VR が無ければ novr | — |

依存は**必ず下向き**、かつ原則として 1 つ下の層まで（`os ← vr ← input ← overlay ← ui ← main`）。overlay は `vr` の型（`Pose` 等）を共通語彙として使ってよい。
`logger` は例外で、どの層からも直接依存してよい唯一の横断クレート（`logger` 自身は他の自前クレートに依存しない）。

## ログ

- ログは**必ず** `logger` のマクロで出す。`println!` / `eprintln!` は使わない（テストハーネスの出力は除く）。
  ```rust
  logger::warn!("パネルの描画を取得できない", error = e, panel = ?panel); // key = Display, key = ?Debug
  ```
- 出力は 1 行 1 件の logfmt: `ts=2026-10-13T09:00:00.123Z level=warn target=ui msg=パネルの描画を取得できない error="device lost" panel=0`。
  `ts level target msg` は固定、以降はフィールドを書いた順。キーは snake_case。`target` は `module_path!()`。
- 出力先は `main` だけが組み立てる: 標準エラー出力（`logger::StderrSink`）と `os::log_dir()/nyaw.log`（`os::FileSink`。Windows `%LOCALAPPDATA%\nyaw\logs`、Linux `$XDG_STATE_HOME/nyaw/logs`）。
  起動ごとに新しく作り、前回分は `nyaw-prev.log` に 1 世代だけ残す（既存の prev は消してから退避）。
  ファイル入出力は OS 層の責務なので、新しい出力先が要るときも `Sink` の実装は `os` に置く。
- レベルは環境変数 `NYAW_LOG`（`trace`/`debug`/`info`/`warn`/`error`、既定 `info`）。`init` 前のログは stderr・info 以上に出る。
- 外部クレートの `log` 出力（gpui 等）も同じロガーに流れる。パニックも `main` のフックで `error` として記録する。

## ランタイム（設計思想）

novr・SteamVR (OpenVR)・OpenXR はどれも対等な「手段」でしかない。novr はテスト用の抜け道ではなく、
ランタイム層（`crates/vr`）に OpenVR / OpenXR と並んで常に存在し、ビルドしたバイナリでも常に使える。

- **基本操作はどのランタイムでもネイティブに行える**こと。VR 固有の機能（手・コントローラ機種・パネルを掴んで動かす等）
  が novr で使えないのは構わない。
- ランタイムごとの違いは `VrBackend::surface()` が返す `Surface` だけで表し、上位層はそれ以外で分岐しない。
  - `Surface::Overlay`（OpenVR / OpenXR）: パネルは非表示ウィンドウで描き、読み戻して VR のオーバーレイへ送る。入力はコントローラのレーザー。
  - `Surface::Desktop`（novr）: パネルは OS のウィンドウとして表示する。入力は OS のマウスがネイティブに届く。
- ビューはランタイムを意識しない（どのランタイムでも同じビューが動く）。

## 1 フレームの流れ（`ui::run`。どのランタイムでも同じ。novr ではコントローラが無いので入力は空、送信もしない）

```
ui::run（GPUI のイベントループ上で約 90Hz）/ testkit の VrKit::step
 └ ui::pump → overlay.tick(vr)
     ├ vr.poll()            → Frame { hmd, controllers: Vec<RawController> }
     ├ input::map(raw)      → ControllerState { aim, select, grab, menu, scroll }
     ├ レーザー × パネル判定 → Vec<UiEvent { panel, hand, PointerEvent }>
     └ vr.set_overlay_pose  （掴み移動を反映）
 └ PointerBridge::translate → gpui::PlatformInput → window.dispatch_event
 └ ui::capture(window)      → Frame（RGBA、デバイスピクセル）   ※Surface::Overlay のみ。testkit は testkit::raster::capture
 └ overlay.submit(vr, ...)  → vr.submit_overlay_pixels
```

パネル用ウィンドウは `ui::open_panel_window(cx, surface, ..)` で開く。`Overlay` では非表示で、
`Window::render_to_image` で画面に出さずに描画して読み戻す。`Desktop` では普通に表示する。ルートビューを借用したまま描画・入力すると
二重借用で落ちるので、ウィンドウ操作は `WindowHandle::update` ではなく `cx.update_window(handle.into(), ..)` を使う。

## 規約

- 語彙は OpenXR に合わせる: プロファイル `"valve/index_controller"`、入力 `"trigger/value"`。OpenVR バックエンドが翻訳する。
- 座標: トラッキング空間はメートル・前方 -Z。パネル表面はローカル +Z。パネルのピクセル座標は左上原点。
- 新しいコントローラ対応 = `crates/input` の `PROFILES` に 1 行追加。上位層でプロファイル名を見ない。
- 新しいランタイム = `crates/vr` に `VrBackend` 実装を追加し `connect` に登録。`surface()` で見せ方を宣言する。
- `CODE_OF_CONDUCT.md` に従う: コメントは日本語、Conventional Commits、CalVer（現在 `2026.0.1`、`[workspace.package]` で一括管理）、単体テスト必須、層をまたぐものは結合テスト。
- `// ponytail:` コメントは意図的な簡略化。限界と次の一手が書いてある。

## テスト

`cargo test --workspace`。ビルドには cmake と C++ コンパイラが要る（`vr` が OpenXR ローダーを静的リンクし、`openvr_bindings` もビルド時に OpenVR を cmake でビルドするため）。cmake が PATH に無ければ環境変数 `CMAKE` で指す（Windows は Visual Studio 同梱の `Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe` でよい）。CI は `cargo fmt --check` と `cargo clippy --workspace --all-targets -D warnings`。

- **単体テスト**: 各クレート内の `#[cfg(test)]`。その層の範囲だけで完結させる。実機の代わりは `vr` の `test-support` フィーチャーで出る `vr::mock::MockBackend`。単体テストから testkit は使わない（依存が循環する）。
- **ユーザー操作のシナリオ（E2E 風）**: `crates/testkit/tests/` に置く。要素は GPUI の `.debug_selector(|| "名前".into())` で指定し、プロファイル名や座標計算はテストに書かない。
  - **基本操作**（クリック・ホバー・見えている色など）: `Driver` に対して1回だけ書き、`on_every_runtime!` で全ランタイム（novr / VR）で実行する（例: `home.rs`）。
  - **UI のロジックだけ**の確認: ランタイムに関係しないので `NoVrKit` だけで書く。
  - **VR 固有の機能**（レーザー・機種・手・パネル移動・VR に映る描画）: `VrKit` で書く（例: `vr_controls.rs`, `panel_texture.rs`）。
  - `color_at` は「ユーザーに見えている色」: novr は画面、VR は VR パネルに送られたテクスチャ。テストプラットフォームには GPU が無いため、描画は `testkit::raster` の CPU ラスタライザ（単色矩形のみ）。
- **実 GPU での読み戻し**: `crates/ui/tests/gpu_capture.rs`（`harness = false`、本番と同じプラットフォームで非表示ウィンドウを描いて読み戻す。Windows のみ）。
- **実ランタイムでの疎通確認**: `cargo run -p vr --example smoke`（OpenXR は `-- --openxr`。手動。SteamVR に接続し、オーバーレイに送った画像をランタイムから読み戻して確かめる）。HMD が無ければ SteamVR 同梱の null ドライバを使う: `{"steamvr": {"requireHmd": false, "forcedDriver": "null", "activateMultipleDrivers": true}, "driver_null": {"enable": true}}` を書いた `steamvr.vrsettings` だけを置いたフォルダを `VR_CONFIG_PATH` で指す（普段の設定には触らない）。SteamVR がすでに起動していると、そちらの設定で動くので先に終了しておく。
- **OpenXR（Monado）での確認**: Windows 版 Monado は公式の配布物が無いが、Monado の GitLab CI の `windows` ジョブの成果物（`install/`）に`monado-service.exe` と `openxr_monado.json` が入っている（`https://gitlab.freedesktop.org/api/v4/projects/2685/jobs/<job id>/artifacts`）。`XRT_COMPOSITOR_COMPUTE=false` で `monado-service.exe` を起動し（Windows 版はコンピュート合成でクアッドを出すと落ちる）、`XR_RUNTIME_JSON` で `openxr_monado.json` を指して smoke を `-- --openxr` で流す。模擬 HMD（simulated）の画面にパネルが映る。読み戻しの手段は無い。

```rust
// 基本操作: 全ランタイムで実行（click_button::novr / click_button::vr）
fn click_button(kit: &mut impl Driver<Home>) {
    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
on_every_runtime!(click_button, |_, _| Home::default());

// VR 固有
#[gpui::test]
fn left_touch_controller_clicks(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Home::default());
    kit.hand(Hand::Left).profile("oculus/touch_controller").click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
```

## 未実装（土台のみ）

- 実ランタイムでの確認の残り。OpenVR（`openvr_bindings` の C API の関数表。openvr_api は jsoncpp が OpenXR ローダーと衝突するのでリンクせず、`openvr/client.rs` が vrclient を直接読む）は null ドライバで接続・オーバーレイへの送信（Vulkan テクスチャ、毎フレーム更新）まで確認済みで、コントローラ入力は未確認。OpenXR（XR_EXTX_overlay + XR_KHR_vulkan_enable2）は Monado（Windows、simulated）でパネルが映るところまで確認済みで、コントローラ入力は未確認。
- Linux での描画の読み戻し（gpui の `render_to_image` が Windows のみ実装。`gpui_wgpu` のヘッドレスレンダラ経由が候補）。
- 描画は毎フレーム CPU に読み戻し、`vr` が Vulkan のイメージへ上げ直してランタイムへ渡す（`crates/vr/src/gpu.rs`。1024x768 で約 1.6ms）。GPUI のテクスチャを読み戻さずに直接共有する経路は未実装。
- Windows での OpenXR ランタイム検出（レジストリ）。
- `gpui` は crates.io 版では `Window::dispatch_event` を外部から呼べないため、Zed の git リビジョンに固定している。`render_to_image` のため `ui` は本番でも gpui の `test-support` フィーチャーを有効にしている。
