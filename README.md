# nyaw (にゃう)

VR オーバーレイアプリ

## ビルドと起動

必要なもの: 
- Rust (stable)
- cmake
- C++ コンパイラ

```sh
cargo run --release              # VR ランタイムを自動検出（無ければウィンドウで動くエミュレーターで起動）
cargo run --release -- --openvr  # SteamVR を使う（--openxr で OpenXR、--novr でエミュレーター）
```

## ライセンス

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)

絵文字は [Twemoji](https://github.com/jdecked/twemoji) v17.0.3 の SVG を同梱しています。
Graphics: Twitter, Inc. and contributors / [CC BY 4.0](crates/design/twemoji/LICENSE-GRAPHICS)。
