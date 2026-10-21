# nyaw (にゃう)

VR オーバーレイアプリ。SteamVR（OpenVR）と OpenXR に対応。
現在の OS バックエンドは Windows のみです。OS 固有の実装はサブシステムに分離しています。

## ビルドと起動

必要なもの: Rust（stable）、cmake と C++ コンパイラ。

```sh
cargo run --release              # VR ランタイムを自動検出（無ければウィンドウで動くエミュレーターで起動）
cargo run --release -- --openvr  # SteamVR を使う（--openxr で OpenXR、--novr でエミュレーター）
```

## 画面・ウィンドウ配信

Testing の「画面・ウィンドウ配信」から対象を選ぶと、別のパネルに表示します。
ポインタで押すと OS のタッチ入力、押したまま動かすとスワイプになります。
左右の手は独立した接点です。配信画面の「配信を停止」で閉じます。

Windows は WGC を使います。ウィンドウのタッチ操作は、その窓がデスクトップ上で見えている必要があります。
最初のタッチで前面化を試み、対象が別の窓に隠れている場合は入力を止めてエラーを表示します。

現在の画像転送は CPU に読み戻した RGBA を使用します。

バックエンドとカーネルのテスト:

```sh
cargo test -p desktop -p kernel -p overlay -p subsystems --no-default-features --lib
```

実デスクトップや UI を通す E2E は通常実行ではスキップされます。実行可能な環境で明示的に指定します。

```sh
cargo test -p desktop --test windows_capture -- --ignored
cargo test -p testkit --test desktop_stream -- --ignored
```

## 設定

`config.toml`（Windows は `%LOCALAPPDATA%\nyaw\`）に書く。

```toml
lod_quality = 2 # 遠くのパネルの解像度を下げる度合い。0 = Off、1 = Low、2 = Mid（既定）、3 = High
```

ログは `%LOCALAPPDATA%\nyaw\logs\nyaw.log` に出る。

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
