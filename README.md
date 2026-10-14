# nyaw (にゃう)
VR オーバーレイアプリ

## 開発・実行

必要なもの: Rust (edition 2024 対応の stable)、cmake と C++ コンパイラ（OpenVR / OpenXR のビルドに使う）。
Node.js 22 以降（Web のページ `web/` を build.rs が Vite でビルドする。`NYAW_WEB_SKIP=1` で飛ばせる）、
Linux では WebKitGTK（`libwebkit2gtk-4.1-dev`。Web のパネルの Tauri が使う）も要る。

```sh
cargo run                  # VR ランタイムを自動検出。無ければ novr（VR をウィンドウで模すエミュレーター）で起動
cargo run -- --novr        # novr 固定
cargo run -- --openxr      # OpenXR 固定（--openvr で SteamVR）
cargo run -- --profile     # プロファイラ付き（ランタイム指定と併用可。--profile=PATH でダンプ先を指定）
NYAW_LOG=debug cargo run   # ログレベル変更（trace/debug/info/warn/error、既定 info）
```

ログは標準エラーと `%LOCALAPPDATA%\nyaw\logs\nyaw.log`（Linux は `$XDG_STATE_HOME/nyaw/logs`）に出る。

`config.toml`（Windows は `%LOCALAPPDATA%\nyaw\config.toml`）の `lod_quality` で、距離に応じたパネルの描画解像度を調整できる。
`0` は Off（常に LOD0）、`1` は Low、`2` は Mid（既定）、`3` は High。
LOD0〜3 の縦横の解像度は元の 1、1/2、1/4、1/8。パネルの物理サイズと入力座標は変わらない。
解像度の端数は切り捨てるため、8 の倍数でないサイズでは低 LOD の表示領域に最大 7 論理ピクセルの丸めが生じる。
カメラからパネル中心までの距離で切り替え、Low は 1 / 2 / 4 m、Mid は 2 / 4 / 8 m、High は 4 / 8 / 16 m からそれぞれ LOD1 / 2 / 3 になる。

```toml
lod_quality = 0 # 距離による解像度の低下を無効にする（省略時は 2）
```

VR パネルは中央をアンカーに、サイズ 75%→100%・不透明度 0%→100% で表示し（200ms）、閉じると逆方向に消える（150ms）。
`animato` で実時間を使って進め、最初の描画が届いてから開始する。途中で閉じても現在の状態から消え、入力は即座に止まる。
各インスタンスで `PanelSpec::without_animation()` を指定すると無効化できる（GPUI・Web 共通）。
作成後は `PanelCommand::Animated(false)`（Web は `nyaw.command({ animated: false })`）でも切り替えられる。
Testing の Animation ボタンでも自分のパネルだけ切り替えられる。レーザー・カーソル・ドラッグ表示は既定で無効。
フェードは OpenVR の `SetOverlayAlpha`、OpenXR の `XR_KHR_composition_layer_color_scale_bias` で合成し、再生中の画像の再転送を避ける。
不透明度 API の無いランタイムだけ、加工前の描画を保持して RGBA を減衰させて転送する。デスクトップの OS ウィンドウは対象外。

`--profile` を付けると、FPS・フレーム時間・CPU/GPU の内訳・ドロップフレームをグラフと数値で出すプロファイラパネルが開き（Testing パネルの Profiler ボタンでも開閉・計測の有効/無効を切り替えられる。パネルの Close で止まる）、
1 フレーム 1 行の CSV を同じログフォルダの `profile.csv` に書き出す（前回分は `profile-prev.csv`）。

### テスト

E2E はローカルで実行する。CI では lint・整形・型チェック・ビルドのみを行う。

```sh
cargo test --workspace     # novr 上の E2E を含む（実ランタイム不要）
cargo test -p ui --test panel_lod -- --ignored # Windows の GPUI / Web で LOD と入力を確認
cargo test -p ui --test panel_lod -- --ignored --off # 0=Off を確認
cargo runtimes list        # 実ランタイム (Monado / SteamVR) の状態
cargo runtimes install     # Monado を導入するウィザード（SteamVR は Steam 側で入れる）
cargo test -p testkit --test runtimes   # 実ランタイムでの E2E
cargo test -p overlay --test animation_runtime -- --ignored # 起動済み SteamVR でアニメーション画像を検証（HMD 必須）
```

### Web のページ（`web/`）

Testing（GPUI）の「Web Testing」ボタンで、Testing の Web 版（React + Tailwind + Base UI）を Tauri の WebView で開ける。

```sh
cd web
npm ci
npm run dev        # ブラウザで見た目を確かめる（カーネルとは繋がらない）
npm run check      # oxlint・oxfmt・tsc・ビルド（CI の web ジョブと同じ）
npm run fmt        # oxfmt で整形
npx playwright install chromium
npm test           # 事前に npm run build。入力・スクロール・ルーティングの回帰テスト（ローカルのみ）
```

Web Testing の「Pointer test」でマウス／タッチ方式を切り替え、タップ・ドラッグ・スワイプと、スティックの縦横スクロールを試せる。

Web のパネルの E2E は実際の GPUI のウィンドウと WebView を開くので、Linux は画面（Xvfb でよい）があるときだけ流れる。

```sh
Xvfb :99 & DISPLAY=:99 cargo test -p ui --tests
```

環境変数 `NYAW_E2E_WATCH` を付けると、E2E 中のパネルをパネルごとのウィンドウで見られる。値は 1 フレームの表示時間（ms、数値でなければ 100）。
見えるのはテスト用ラスタライザの絵（矩形のみ、文字は出ない）。並列だと窓が乱立するので `--test-threads=1` 推奨。

```sh
NYAW_E2E_WATCH=300 cargo test -p testkit --test testing -- --test-threads=1
```

構成と設計は [AGENT.md](AGENT.md) を参照。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選んで使える。
