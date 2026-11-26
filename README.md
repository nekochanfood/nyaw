<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logos/Logo_Full_Dark.svg">
    <img src="assets/logos/Logo_Full_Light.svg" alt="nyaw" width="320">
  </picture>
</p>

<h1 align="center">nyaw (にゃう)</h1>

<p align="center">VR オーバーレイアプリ</p>

## ビルドと起動

### 前提環境

ビルドには以下の環境が必要です。

- Rust (stable)
- cmake（OpenXR ローダーおよび OpenVR バインディングのビルドに使用）
- C++ コンパイラ

> [!NOTE]
> Windows 環境で cmake が `PATH` に通っていない場合は、環境変数 `CMAKE` に実行ファイルの絶対パス（例: Visual Studio 同梱の `cmake.exe`）を指定してください。

### 起動コマンド

```sh
cd apps/nyaw

# VR ランタイムを自動検出して起動
cargo run --release

# SteamVR (OpenVR) を指定して起動
cargo run --release -- --openvr

# OpenXR を指定して起動
cargo run --release -- --openxr

# VR 機器を使わずエミュレーター（novr）で起動
cargo run --release -- --novr
```

## ライセンス

本ソフトウェアは [MIT ライセンス](LICENSE-MIT) または [Apache-2.0 ライセンス](LICENSE-APACHE) の下で配布されています。

絵文字グラフィックには [Twemoji](https://github.com/jdecked/twemoji) v17.0.3 の SVG を同梱しています。  
Graphics: Twitter, Inc. and contributors / [CC BY 4.0](apps/nyaw/crates/design/twemoji/LICENSE-GRAPHICS)。
