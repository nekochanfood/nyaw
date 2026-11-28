<p align="center">
  <img src="assets/animations/logo-full/logo-full-light.webp" alt="nyaw" width="364">
</p>
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

nyaw アプリとウェブサイトは [GPL-3.0-or-later](LICENSE-GPL)（[追加許可](LICENSE-EXCEPTION)あり）、再利用できる基盤のクレートは [MIT ライセンス](LICENSE-MIT) または [Apache-2.0 ライセンス](LICENSE-APACHE) の下で配布されています。

| ライセンス | 対象 |
|---|---|
| GPL-3.0-or-later | `apps/web`、`apps/nyaw` のアプリ本体（`src`、`build.rs`、`installer`）、`apps/nyaw/crates` の `ui`・`design`・`i18n`・`dashboard`・`settings`・`setup`・`wrist`・`toast`・`beam`・`updater`・`guide`・`testing`・`testkit` |
| MIT OR Apache-2.0 | 上記以外（`subsystems`、`kernel`、`overlay`、`logger`、`animation`、`lottie`、`profiler`、`metrics` など） |

「nyaw」の名前と、ロゴ・アイコン・ロゴのアニメーション（`assets/logos`、`assets/icons`、`assets/animations/logo-full` の JSON と WebP）は、上の表のライセンスの対象外で、[ブランド資産の利用条件](LICENSE-BRAND)が適用されます。改変していない公式版の再配布や、nyaw を紹介するための使用は許可しています。改変版を配布または公開する場合は、名前とロゴを差し替えてください。

同梱する第三者の素材とソース（`apps/nyaw/vendor`、フォント、アイコン、Twemoji、キーボードのデータなど）には、それぞれの配置先に記載したライセンスが適用されます。`assets/renders` のコントローラ画像は、どちらのライセンスの対象でもありません（[詳細](assets/renders/acknowledgements.md)）。

過去に MIT または Apache-2.0 で公開した版は、引き続きその条件で利用できます。

絵文字グラフィックには [Twemoji](https://github.com/jdecked/twemoji) v17.0.3 の SVG を同梱しています。  
Graphics: Twitter, Inc. and contributors / [CC BY 4.0](apps/nyaw/crates/design/twemoji/LICENSE-GRAPHICS)。
