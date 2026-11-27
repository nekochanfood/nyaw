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
- **`VrKit`**: 6 自由度 VR 空間のシミュレーションキット。コントローラ把持（`hand`）、機種プロファイル切り替え（`profile`）、ランタイムが報告するレンダーモデル名の偽装（`render_model`。操作ガイドの機種判定の検証）、空間ドラッグ（`drag_panel`）、空間の点への照準（`aim_at`）と掴み移動・角のサイズ変更の追従待ち（`settle`）、および要素の境界取得（`bounds`、`try_bounds`）を扱う。パネルの解像度が変わると（キーボードの `PanelCommand::Resize`、角のサイズ変更）、本番と同じくウィンドウの大きさも合わせる。
- **`DesktopKit`**: パネルウィンドウに対して直接入力を注入し、実ランタイムへの描画転送を検証するキット。

## 実ランタイムを用いた E2E テスト

外部の VR ランタイムプロセスを実際に起動し、本番同等のパイプラインで動作を検証する（`crates/testkit/tests/runtimes.rs`、`#[ignore]` 指定）。

- **実行コマンド**: `cargo test -p testkit --test runtimes -- --ignored [monado|steamvr]`
- **ライフサイクル管理**: `testkit::runtimes` がランタイムプロセスを自動起動し、テスト完了時に安全に停止・クリーンアップする。既存の起動中ランタイムが存在する場合は設定破壊を防ぐためテストをスキップする。
- **環境構築ツール (`cargo runtimes`)**: テストに必要なランタイムの取得・インストールを行うヘルパーコマンド（保存先: `%LOCALAPPDATA%\nyaw\runtimes`）。
  - **Monado**: GitLab CI の Windows ビルド成果物を取得し、シミュレーション HMD モードで起動して OpenXR レイヤーの動作を検証する。
  - **SteamVR**: null ドライバを有効化した独立の一時設定（`VR_CONFIG_PATH`）を指定して起動し、物理 HMD が接続されていない環境でもオーバーレイ転送と読み戻しを検査する。
