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
- PipeWire と libxkbcommon の開発用ファイル、および libclang（画面の配信と OS キーボードに使用。Arch Linux では `pipewire`、`libxkbcommon`、`clang`、Debian / Ubuntu では `libpipewire-0.3-dev`、`libxkbcommon-dev`、`libclang-dev`）

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

## 対応状況

🟢 は対応済み、🟡 は一部のみ対応または未検証、🔴 は未対応、- は仕様上対応できないことを表します。各 OS の「共通実装」は VR ランタイムに依存しない部分（画面、ロジック、OS との連携）の対応状況を、「OpenVR」と「OpenXR」はその OS でそのランタイムを使用したときの対応状況を表します。

<table>
<thead>
<tr><th rowspan="2">機能</th><th colspan="3">Windows</th><th colspan="3">Linux<a href="#note-linux"><sup>*1</sup></a></th></tr>
<tr><th>共通実装</th><th>OpenVR</th><th>OpenXR<a href="#note-openxr"><sup>*2</sup></a></th><th>共通実装</th><th>OpenVR</th><th>OpenXR<a href="#note-openxr"><sup>*2</sup></a></th></tr>
</thead>
<tbody>
<tr><td>オーバーレイとしてのパネルの表示</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>VR 機器なしでの動作（novr エミュレーター）</td><td align="center">🟢</td><td align="center">-</td><td align="center">-</td><td align="center">🟡<a href="#note-novr"><sup>*3</sup></a></td><td align="center">-</td><td align="center">-</td></tr>
<tr><td>コントローラによるパネルの操作（レーザー・クリック・スクロール）</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟡</td></tr>
<tr><td>パネルの掴み移動と、手・HMD への取り付け</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟡</td></tr>
<tr><td>パネルのトラッカーへの取り付け</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡<a href="#note-openxr-ext"><sup>*4</sup></a></td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟡<a href="#note-openxr-ext"><sup>*4</sup></a></td></tr>
<tr><td>ハンドトラッキングによる操作</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡<a href="#note-openxr-ext"><sup>*4</sup></a></td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟡<a href="#note-openxr-ext"><sup>*4</sup></a></td></tr>
<tr><td>手首の時計</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟢</td><td align="center">🟡</td><td align="center">🟡</td></tr>
<tr><td>ダッシュボードとクイック設定</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>設定画面</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>セットアップウィザード</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>仮想キーボードによる nyaw の画面への文字入力</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>仮想キーボードによる他のアプリへのキー入力</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡<a href="#note-linux-desktop"><sup>*7</sup></a></td><td align="center">🟡</td><td align="center">🟡</td></tr>
<tr><td>OS の通知のトースト表示</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🔴</td><td align="center">🔴</td><td align="center">🔴</td></tr>
<tr><td>画面・ウィンドウの配信</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡<a href="#note-linux-desktop"><sup>*7</sup></a></td><td align="center">🟡</td><td align="center">🟡</td></tr>
<tr><td>配信中の画面・ウィンドウへの操作</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡<a href="#note-linux-desktop"><sup>*7</sup></a></td><td align="center">🟡</td><td align="center">🟡</td></tr>
<tr><td>アプリの一覧と起動</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🔴</td><td align="center">🔴</td><td align="center">🔴</td></tr>
<tr><td>再生中のメディアの表示と操作</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢<a href="#note-linux-media"><sup>*8</sup></a></td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>空間音響（Steam Audio の HRTF）</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>視野に合わせた HUD の配置</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>距離に応じた描画解像度の調整（LOD）</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>文字サイズの変更と太字化</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>表示言語の切り替え（日本語・英語）と OS の言語の検出</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟡</td></tr>
<tr><td>コントローラなどの電池残量の表示</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">-<a href="#note-runtime-api"><sup>*5</sup></a></td><td align="center">🟢</td><td align="center">🟡</td><td align="center">-<a href="#note-runtime-api"><sup>*5</sup></a></td></tr>
<tr><td>ランタイムの起動に合わせた自動起動</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">-<a href="#note-runtime-api"><sup>*5</sup></a></td><td align="center">🟢</td><td align="center">🟡</td><td align="center">-<a href="#note-runtime-api"><sup>*5</sup></a></td></tr>
<tr><td>ダッシュボード表示中のゲームへの入力のブロック</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">-<a href="#note-runtime-api"><sup>*5</sup></a></td><td align="center">🟢</td><td align="center">🟡</td><td align="center">-<a href="#note-runtime-api"><sup>*5</sup></a></td></tr>
<tr><td>アップデートの確認と適用</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🟢</td><td align="center">🔴<a href="#note-update"><sup>*6</sup></a></td><td align="center">🔴</td><td align="center">🔴</td></tr>
</tbody>
</table>

<a name="note-linux"></a>\*1 Linux では、HMD を接続しない SteamVR（null ドライバ）にオーバーレイとして接続し、パネルが SteamVR のコンポジタに合成されることを確認しました。共通実装の列は、エミュレーターを用いたテストで検証しています。実機の HMD とコントローラは未検証であり、OpenVR の列ではそれらを必要とする機能を 🟡 としています。🔴 の機能は、OS との連携（通知、アプリの一覧など）を Windows でのみ実装しています。

<a name="note-openxr"></a>\*2 OpenXR のオーバーレイには `XR_EXTX_overlay` に対応したランタイム（Monado など）が必要です。SteamVR の OpenXR ランタイムはこの拡張に対応していないため、SteamVR では `--openxr` を指定しても OpenVR で接続します。OpenXR は Windows の Monado（HMD のシミュレーション）でのみ検証しており、実機のコントローラと Linux での動作は未検証です。

<a name="note-novr"></a>\*3 Linux ではエミュレーターのウィンドウの表示までを確認しています。エミュレーターのウィンドウへのマウスとキーボードの入力は未検証です。

<a name="note-openxr-ext"></a>\*4 ランタイムが対応する拡張（トラッカーは `XR_HTCX_vive_tracker_interaction`、ハンドトラッキングは `XR_EXT_hand_tracking`）を提供する場合に限り動作します。

<a name="note-runtime-api"></a>\*5 OpenXR には、デバイスの電池残量の取得、ランタイムへのアプリの自動起動の登録、他のアプリへの入力の遮断を行う標準の API がありません。

<a name="note-update"></a>\*6 アップデートは Windows のインストーラーで適用します。Linux 向けの配布物はまだないため、Linux では更新を確認しません。

<a name="note-linux-desktop"></a>\*7 Linux で配信できるのはディスプレイ（画面）のみで、個別のウィンドウは配信できません。表示サーバーに応じて、X11 では XSHM と XTest、wlroots 系のコンポジタでは ext-image-copy-capture（無ければ wlr-screencopy）と仮想ポインタ・仮想キーボード、その他の Wayland（KDE Plasma、GNOME など）では xdg-desktop-portal と PipeWire を使います。portal では初回にデスクトップに表示される確認画面で、共有するディスプレイを選んで許可する必要があります（パネルには許可待ちである旨を表示します）。X11、KDE Plasma、niri、Hyprland、ShojiWM で検証しました。wlr-screencopy で取り込む場合は OS のカーソルの画像を取得できないため、nyaw のポインタを表示します。portal ではキー配列に無い文字を入力できない場合があります。

<a name="note-linux-media"></a>\*8 Linux では、MPRIS（D-Bus）に対応したプレイヤーを表示して操作します。KDE Plasma で Spotify と Chromium 系のブラウザ（Helium）を使い、情報の取得を検証しました。再生・一時停止、シーク、曲送り、曲戻しは Spotify で検証しました。アプリの画面を前面に出す操作は、プレイヤーへの要求が届くことまでを確認しており、ウィンドウが前面に出ることは未検証です。アプリのアイコンは、`.desktop` ファイルが指す PNG（絶対パス、hicolor テーマ、pixmaps）がある場合に表示します。

## ライセンス

本ソフトウェアは [MIT ライセンス](LICENSE-MIT) または [Apache-2.0 ライセンス](LICENSE-APACHE) の下で配布されています。

絵文字グラフィックには [Twemoji](https://github.com/jdecked/twemoji) v17.0.3 の SVG を同梱しています。  
Graphics: Twitter, Inc. and contributors / [CC BY 4.0](apps/nyaw/crates/design/twemoji/LICENSE-GRAPHICS)。
