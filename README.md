# nyaw (にゃう)
VR オーバーレイアプリ

## 開発・実行

必要なもの: Rust (edition 2024 対応の stable)、cmake と C++ コンパイラ（OpenVR / OpenXR のビルドに使う）。
Linux では WebKitGTK（`libwebkit2gtk-4.1-dev`。Web UI の Tauri が使う）も要る。

```sh
cargo run                  # VR ランタイムを自動検出。無ければ novr（デスクトップのウィンドウ）で起動
cargo run -- --novr        # novr 固定
cargo run -- --openxr      # OpenXR 固定（--openvr で SteamVR）
cargo run -- --profile     # プロファイラ付き（ランタイム指定と併用可。--profile=PATH でダンプ先を指定）
cargo run -- --webui       # GPUI の代わりに Tauri の WebView で画面を出す（ランタイム指定と併用可）
NYAW_LOG=debug cargo run   # ログレベル変更（trace/debug/info/warn/error、既定 info）
```

ログは標準エラーと `%LOCALAPPDATA%\nyaw\logs\nyaw.log`（Linux は `$XDG_STATE_HOME/nyaw/logs`）に出る。

`--profile` を付けると、FPS・フレーム時間・CPU/GPU の内訳・ドロップフレームをグラフと数値で出すプロファイラパネルが開き（Testing パネルの Profiler ボタンでも開閉・計測の有効/無効を切り替えられる。パネルの Close で止まる）、
1 フレーム 1 行の CSV を同じログフォルダの `profile.csv` に書き出す（前回分は `profile-prev.csv`）。

### テスト

```sh
cargo test --workspace     # novr 上の E2E を含む（実ランタイム不要）
cargo runtimes list        # 実ランタイム (Monado / SteamVR) の状態
cargo runtimes install     # Monado を導入するウィザード（SteamVR は Steam 側で入れる）
cargo test -p testkit --test runtimes   # 実ランタイムでの E2E
```

Web UI（`--webui`）の E2E は実際の WebView を開くので、Linux は画面（Xvfb でよい）があるときだけ流れる（novr 側は xdotool も要る）。

```sh
Xvfb :99 & DISPLAY=:99 cargo test -p webui --tests
```

環境変数 `NYAW_E2E_WATCH` を付けると、E2E 中のパネルをパネルごとのウィンドウで見られる。値は 1 フレームの表示時間（ms、数値でなければ 100）。
見えるのはテスト用ラスタライザの絵（矩形のみ、文字は出ない）。並列だと窓が乱立するので `--test-threads=1` 推奨。

```sh
NYAW_E2E_WATCH=300 cargo test -p testkit --test testing -- --test-threads=1
```

構成と設計は [AGENT.md](AGENT.md) を参照。
