# crates/testkit

層: 7 テスト

Playwright の設計思想を取り入れた宣言的な E2E テスト基盤を提供する。開発および CI 検証専用のクレートであり、本番バイナリには含まれない。

各 VR ランタイム（novr、OpenVR、OpenXR）に対して均一な抽象化キットを提供する。

## 主な公開 API

### 全キット共通インターフェース（Driver）

```rust
pub trait Driver<V> {
    fn click(&mut self, selector: &str);
    fn hover(&mut self, selector: &str);
    fn down(&mut self, selector: &str);
    fn up(&mut self, selector: &str);
    fn drag(&mut self, from: &str, to: &str);
    fn scroll(&mut self, selector: &str, delta_y: f32);
    fn read<T>(&mut self, f: impl FnOnce(&V) -> T) -> T;
    fn color_at(&mut self, selector: &str) -> Rgba;
}
```

マクロ `on_every_runtime!(test_fn, view_factory)` を用いることで、同一のテストシナリオを novr および VR の全ランタイムに対して網羅実行できる。

### ランタイム別テストキット

- **`NoVrKit`**: デスクトップエミュレーターを用いたテストキット。ヘッドレス実行、画面全体のスクリーンショット検証（`screenshot`、`color_on_screen`）、およびマウス・キー入力注入を提供する。
- **`VrKit`**: 6 自由度 VR 空間のシミュレーションキット。コントローラ把持（`hand`）、機種プロファイル切り替え（`profile`）、空間ドラッグ（`drag_panel`）、空間の点への照準（`aim_at`）と掴み移動・角のサイズ変更の追従待ち（`settle`）、および要素の境界取得（`bounds`、`try_bounds`）を扱う。パネルの解像度が変わると（キーボードの `PanelCommand::Resize`、角のサイズ変更）、本番と同じくウィンドウの大きさも合わせる。
- **`DesktopKit`**: パネルウィンドウに対して直接入力を注入し、実ランタイムへの描画転送を検証するキット。

## 実ランタイムを用いた E2E テスト

外部の VR ランタイムプロセスを実際に起動し、本番同等のパイプラインで動作を検証する（`crates/testkit/tests/runtimes.rs`、`#[ignore]` 指定）。

- **実行コマンド**: `cargo test -p testkit --test runtimes -- --ignored [monado|steamvr]`
- **画面の配信の検証 (`NYAW_E2E_DESKTOP`)**: `1` を指定すると、ランタイムが取り込める形式でこのセッションの画面を配信し（Wayland では portal 経由の DMA-BUF）、フレームをランタイムへ送って読み戻す。XWayland の帯を塗り替え続けて画面を動かし、1 フレームの送信時間を出力する。`rgba` を指定すると、比較のため RGBA で送る。SteamVR は Wayland なしで起動するため、この検証の間だけ `WAYLAND_DISPLAY` を戻す。
- **ライフサイクル管理**: `testkit::runtimes` がランタイムプロセスを自動起動し、テスト完了時に安全に停止・クリーンアップする。既存の起動中ランタイムが存在する場合は設定破壊を防ぐためテストをスキップする。
- **環境構築ツール (`cargo runtimes`)**: テストに必要なランタイムの取得・インストールを行うヘルパーコマンド（保存先: `%LOCALAPPDATA%\nyaw\runtimes`）。
  - **Monado**: GitLab CI の Windows ビルド成果物を取得し、シミュレーション HMD モードで起動して OpenXR レイヤーの動作を検証する。
  - **SteamVR**: null ドライバを有効化した独立の一時設定（`VR_CONFIG_PATH`）を指定して起動し、物理 HMD が接続されていない環境でもオーバーレイ転送と読み戻しを検査する。Linux のコンポジタは HMD を direct mode でのみ駆動するが、null ドライバの HMD には対応するディスプレイがない。そのため一時設定で direct mode を無効にし、ウィンドウへのフォールバック（`allowFallbackMirrorWindowLinux`）を許可する。このフォールバックは X11 でのみ動作するため、起動する SteamVR には `SDL_VIDEODRIVER=x11` を設定し、`WAYLAND_DISPLAY` を外す（XWayland を使用する）。
