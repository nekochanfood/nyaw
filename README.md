# nyaw (にゃう)

VR オーバーレイアプリ。SteamVR（OpenVR）と OpenXR に対応。

## ビルドと起動

必要なもの: Rust（stable）、cmake と C++ コンパイラ、Node.js 22 以降。Linux では `libwebkit2gtk-4.1-dev` も要る。

```sh
cargo run --release              # VR ランタイムを自動検出（無ければウィンドウで動くエミュレーターで起動）
cargo run --release -- --openvr  # SteamVR を使う（--openxr で OpenXR、--novr でエミュレーター）
```

## 設定

`config.toml`（Windows は `%LOCALAPPDATA%\nyaw\`、Linux は `$XDG_STATE_HOME/nyaw/`）に書く。

```toml
lod_quality = 2 # 遠くのパネルの解像度を下げる度合い。0 = Off、1 = Low、2 = Mid（既定）、3 = High
```

ログは `%LOCALAPPDATA%\nyaw\logs\nyaw.log`（Linux は `$XDG_STATE_HOME/nyaw/logs`）に出る。

開発者向けの情報は [AGENT.md](AGENT.md) を参照。

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のどちらかを選んで使える。
