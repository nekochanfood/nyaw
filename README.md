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

Linux 環境では、上記に加えて以下が必要です。

- pkg-config と ALSA の開発用ファイル（音声出力に使用。Arch Linux では `alsa-lib`、Debian / Ubuntu では `libasound2-dev`）
- Vulkan に対応した GPU ドライバ（パネルの描画とランタイムへの転送に使用）
- fontconfig（書体を指定しない文字に使う書体の選択に使用）
- curl と unzip（ビルド時に Steam Audio を取得するために使用。取得できない場合、空間音響は左右のパンニングで代替されます）

### 起動コマンド

```sh
# VR ランタイムを自動検出して起動
cargo run --release

# SteamVR (OpenVR) を指定して起動
cargo run --release -- --openvr

# OpenXR を指定して起動
cargo run --release -- --openxr

# VR 機器を使わずエミュレーター（novr）で起動
cargo run --release -- --novr
```

## 対応状況

🟢 は対応済み、🟡 は一部のみ対応または未検証、🔴 は未対応、- は仕様上対応できないことを表します。列のうち「共通実装」は、OS とランタイムに依存しない部分（画面とロジック）の実装状況を表します。

| 機能 | Windows | Linux[<sup>*1</sup>](#note-linux) | 共通実装 | OpenVR | OpenXR[<sup>*2</sup>](#note-openxr) |
|---|:-:|:-:|:-:|:-:|:-:|
| オーバーレイとしてのパネルの表示 | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| VR 機器なしでの動作（novr エミュレーター） | 🟢 | 🟡[<sup>*3</sup>](#note-novr) | 🟢 | - | - |
| コントローラによるパネルの操作（レーザー・クリック・スクロール） | 🟢 | 🟡 | 🟢 | 🟢 | 🟡 |
| パネルの掴み移動と、手・HMD への取り付け | 🟢 | 🟡 | 🟢 | 🟢 | 🟡 |
| パネルのトラッカーへの取り付け | 🟢 | 🟡 | 🟢 | 🟢 | 🟡[<sup>*4</sup>](#note-openxr-ext) |
| ハンドトラッキングによる操作 | 🟢 | 🟡 | 🟢 | 🟢 | 🟡[<sup>*4</sup>](#note-openxr-ext) |
| 手首の時計 | 🟢 | 🟡 | 🟢 | 🟢 | 🟡 |
| ダッシュボードとクイック設定 | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| 設定画面 | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| セットアップウィザード | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| 仮想キーボードによる nyaw の画面への文字入力 | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| 仮想キーボードによる他のアプリへのキー入力 | 🟢 | 🔴 | 🟢 | 🟢 | 🟢 |
| OS の通知のトースト表示 | 🟢 | 🔴 | 🟢 | 🟢 | 🟢 |
| 画面・ウィンドウの配信 | 🟢 | 🔴 | 🟢 | 🟢 | 🟢 |
| 配信中の画面・ウィンドウへの操作 | 🟢 | 🔴 | 🟢 | 🟢 | 🟢 |
| アプリの一覧と起動 | 🟢 | 🔴 | 🟢 | 🟢 | 🟢 |
| 再生中のメディアの表示と操作 | 🟢 | 🔴 | 🟢 | 🟢 | 🟢 |
| 空間音響（Steam Audio の HRTF） | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| 視野に合わせた HUD の配置 | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| 距離に応じた描画解像度の調整（LOD） | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| 文字サイズの変更と太字化 | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| 表示言語の切り替え（日本語・英語）と OS の言語の検出 | 🟢 | 🟢 | 🟢 | 🟢 | 🟢 |
| コントローラなどの電池残量の表示 | 🟢 | 🟡 | 🟢 | 🟢 | -[<sup>*5</sup>](#note-runtime-api) |
| ランタイムの起動に合わせた自動起動 | 🟢 | 🟡 | 🟢 | 🟢 | -[<sup>*5</sup>](#note-runtime-api) |
| ダッシュボード表示中のゲームへの入力のブロック | 🟢 | 🟡 | 🟢 | 🟢 | -[<sup>*5</sup>](#note-runtime-api) |
| アップデートの確認と適用 | 🟢 | 🔴[<sup>*6</sup>](#note-update) | 🟢 | 🟢 | 🟢 |

<a name="note-linux"></a>\*1 Linux では、HMD を接続しない SteamVR（null ドライバ）にオーバーレイとして接続し、パネルが SteamVR のコンポジタに合成されることを確認しました。実機の HMD とコントローラは未検証であり、それらを必要とする機能を 🟡 としています。🔴 の機能は OS との連携（通知、画面の取り込み、入力の注入など）を Windows でのみ実装しています。

<a name="note-openxr"></a>\*2 OpenXR のオーバーレイには `XR_EXTX_overlay` に対応したランタイム（Monado など）が必要です。SteamVR の OpenXR ランタイムはこの拡張に対応していないため、SteamVR では `--openxr` を指定しても OpenVR で接続します。OpenXR は Windows の Monado（HMD のシミュレーション）でのみ検証しており、実機のコントローラは未検証です。

<a name="note-novr"></a>\*3 Linux ではエミュレーターのウィンドウの表示までを確認しています。エミュレーターのウィンドウへのマウスとキーボードの入力は未検証です。

<a name="note-openxr-ext"></a>\*4 ランタイムが対応する拡張（トラッカーは `XR_HTCX_vive_tracker_interaction`、ハンドトラッキングは `XR_EXT_hand_tracking`）を提供する場合に限り動作します。

<a name="note-runtime-api"></a>\*5 OpenXR には、デバイスの電池残量の取得、ランタイムへのアプリの自動起動の登録、他のアプリへの入力の遮断を行う標準の API がありません。

<a name="note-update"></a>\*6 アップデートは Windows のインストーラーで適用します。Linux 向けの配布物はまだないため、Linux では更新を確認しません。

## ライセンス

本ソフトウェアは [MIT ライセンス](LICENSE-MIT) または [Apache-2.0 ライセンス](LICENSE-APACHE) の下で配布されています。

絵文字グラフィックには [Twemoji](https://github.com/jdecked/twemoji) v17.0.3 の SVG を同梱しています。  
Graphics: Twitter, Inc. and contributors / [CC BY 4.0](crates/design/twemoji/LICENSE-GRAPHICS)。
