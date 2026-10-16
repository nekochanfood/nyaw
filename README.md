# nyaw (にゃう)

VR オーバーレイアプリ。SteamVR（OpenVR）と OpenXR に対応。

## ビルドと起動

必要なもの: Rust（stable）、cmake と C++ コンパイラ。

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

## 絵文字

UI では `design::emoji("🐈").size(gpui::px(24.))`、文章では
`design::emoji_text("こんにちは 🐈!")` を `.child(...)` に渡します。
絵文字の丸いボタンには `design::components::emoji_button` を使います。
Testing → 絵文字（Emoji）で表示・折り返し・ボタン操作を確認できます。

## ライセンス

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)

絵文字は [Twemoji](https://github.com/jdecked/twemoji) v17.0.3 の SVG を同梱しています。
Graphics: Twitter, Inc. and contributors / [CC BY 4.0](crates/design/twemoji/LICENSE-GRAPHICS)。
