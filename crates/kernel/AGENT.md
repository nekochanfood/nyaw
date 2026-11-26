# crates/kernel

層: 3 カーネル

アプリケーション全体の設定管理、フレームサイクルの進行、およびパネル操作の集約を担う。最下部の Subsystems の直上に位置し、上位の UI 層および画面クレートはカーネルを通じてのみシステムリソースを操作する。

## 設定管理システム

アプリケーションの設定は `subsystems::os::config_dir()/config.toml` に保存される。実行時の設定値はパネル操作（`PanelCommand`）から分離されており、`Kernel::set(Setting)` で更新し、`Kernel::settings()` で参照する。

設定の優先順位は「コマンドライン引数 > 設定ファイル」である（ログレベルに関しては環境変数 `NYAW_LOG` > 設定ファイル）。使用する VR ランタイムはコマンドライン引数でのみ指定し、設定ファイルには置かない。

### テーブルの決まり

`config.toml` のトップレベルにはテーブルだけを置き、値はすべていずれかのテーブルに入れる。テーブルの名前は、その値を受け取って動作を変えるクレートの名前にする。カーネルが値を保持するだけの設定も、解釈するクレートのテーブルに入れる。テーブルの中のキーは、テーブル名と重なる接頭辞を付けない（`[wrist]` の `scale` など）。

Rust 側では、テーブルごとに構造体（`config::WristTable` など）を 1 つ定義し、`Config` のフィールドにする。値を `Config` に直接置くと、どのクレートの設定かをキーから判別できなくなるためである。この決まりはテスト `every_top_level_key_is_a_crate_table` が検査し、トップレベルにテーブル以外の値があるか、テーブルの名前がワークスペースのクレート名でなければ失敗する。新しい設定を追加するときは、該当するクレートのテーブル構造体にフィールドを追加し、そのクレートのテーブルがまだ無ければ構造体ごと追加する。

`Config` のフィールド、`Setting` の列挙子、`Settings` のフィールドは、どれもルート `AGENT.md` のクレート一覧と同じ層の順に並べる。旧形式（トップレベルにキーを並べた形式）との互換性は持たない。旧形式のキーは未知のキーとして無視され、既定値に戻る。

### 設定項目一覧

各設定項目は `config.toml` のテーブルとキー、対応する `Setting` の列挙子、および既定値を持つ。

| テーブル | キー | 型 / 設定列挙型 | 既定値 | 内容 |
|---|---|---|---|---|
| `[logger]` | `level` | `"trace" \| "debug" \| "info" \| "warn" \| "error"` | `"info"` | ログレベル。環境変数 `NYAW_LOG` があればそちらを優先する。画面からは変更しない。 |
| `[vr]` | `emulator_gpu` | 真偽値 | `true` | novr のエミュレーターの合成に GPU を使うかどうか。`false` にすると CPU で合成する。画面からは変更しない。 |
| `[input.hand]` | `pinch_mm` など | `subsystems::input::HandTuning`<br>`Setting::Hand` | 各既定値 | コントローラを持たない手の判定しきい値とレーザーの揺れ抑制。書いたキーだけが変わる。 |
| `[keyboard]` | `kind` | `"full" \| "tenkeyless" \| "compact"`<br>`Setting::Keyboard` | `"full"` | 仮想キーボードの種類。 |
| `[keyboard]` | `scale` | 浮動小数点数 (`0.5`〜`2.0`)<br>`Setting::KeyboardScale` | `1.0` | 仮想キーボードの表示倍率（0.1 刻み）。 |
| `[keyboard]` | `key_gap` | 浮動小数点数 (`0.0`〜`2.0`)<br>`Setting::KeyGap` | `1.0` | キー間の間隔倍率（0.1 刻み）。 |
| `[keyboard]` | `key_sound` | 真偽値<br>`Setting::KeySound` | `true` | キー押下時の打鍵音の有効化。 |
| `[keyboard]` | `key_sound_volume` | 浮動小数点数 (`0.0`〜`1.0`)<br>`Setting::KeySoundVolume` | `1.0` | 打鍵音の音量（0.1 刻み）。 |
| `[desktop]` | `frame_rate` | 整数 (`1`〜`240` FPS)<br>`Setting::MirrorFrameRate` | `60` | 画面・ウィンドウミラーリングのキャプチャ上限フレームレート。 |
| `[overlay]` | `ui_scale` | 浮動小数点数 (`0.7`〜`1.5`)<br>`Setting::UiScale` | `1.0` | 全パネルの空間寸法の倍率（[アクセシビリティ設定](#アクセシビリティ設定)）。 |
| `[overlay]` | `pointer_smoothing` | 浮動小数点数 (`0.0`〜`0.998`)<br>`Setting::PointerSmoothing` | `0.998` | コントローラレーザーの手振れ補正係数。 |
| `[overlay]` | `pointer_release_deg` | 浮動小数点数 (`0.5`〜`45.0` 度)<br>`Setting::PointerRelease` | `11.25` | 手振れ補正中にレーザーが手から遅れた際、補正強度を半減させる閾値角度。 |
| `[overlay]` | `laser_length` | 浮動小数点数 (`0.05`〜`1.5` m)<br>`Setting::LaserLength` | `0.3` | コントローラから照射されるレーザーの可視長。 |
| `[overlay]` | `cursor_scale` | 浮動小数点数 (`0.5`〜`2.0`)<br>`Setting::CursorScale` | `1.0` | パネル上のカーソル表示倍率（0.1 刻み）。 |
| `[overlay]` | `block_input` | 真偽値<br>`Setting::BlockInput` | `true` | ダッシュボード表示中に背後のゲームに対する入力を遮断するかどうか。 |
| `[overlay]` | `veil_opacity` | 浮動小数点数 (`0.0`〜`1.0`)<br>`Setting::DashboardDim` | `0.5` | 空間全体の暗転幕の濃さ（黒の不透明度、0.1 刻み）。ダッシュボード表示中に背景を暗くする度合いであり、表示中の変更も即座に反映される。 |
| `[kernel]` | `dominant_hand` | `"right" \| "left"`<br>`Setting::DominantHand` | `"right"` | 利き手の設定。デスクトップ配信パネルに対する OS マウス操作権を保持する。 |
| `[kernel]` | `pointer_mode` | `"mouse" \| "touch"`<br>`Setting::PointerMode` | `"touch"` | レーザーのエミュレート方式。設定画面には無く、`Setting::PointerMode` で変更しても書き戻さない。 |
| `[kernel]` | `lod_quality` | 整数 (`0`〜`3`)<br>`Setting::LodQuality` | `2` | 距離 LOD の品質（[パネルの LOD](#パネルの-lod詳細度制御)）。 |
| `[ui]` | `tooltips` | 真偽値<br>`Setting::Tooltips` | `true` | UI 要素に対するツールチップ表示の有効化。 |
| `[ui]` | `tap_sound` | 真偽値<br>`Setting::TapSound` | `true` | ボタン押下時の効果音の有効化。 |
| `[ui]` | `hover_sound` | 真偽値<br>`Setting::HoverSound` | `true` | ボタンにホバーした際の効果音の有効化。 |
| `[design]` | `theme` | `"dark" \| "light"`<br>`Setting::Theme` | `"dark"` | UI の明暗テーマ。`ui::sync_settings` を通じて `design::Mode` に反映される。 |
| `[design]` | `personal_color` | `{ lightness, chroma, hue }`<br>`Setting::PersonalColor` | 未設定 (`None`) | ボタンや強調表示に使用するアクセントカラー。未設定時はテーマの既定色を用いる。 |
| `[design]` | `color_history` | 色の配列（最大 8 件）<br>`Setting::CustomColor` | 空 | ユーザーが作成したカスタムカラーの履歴。新規作成時に先頭へ追加され、重複は除外される。 |
| `[design]` | `text_scale` | 浮動小数点数 (`0.8`〜`1.5`)<br>`Setting::TextScale` | `1.0` | 文字の大きさの倍率（[アクセシビリティ設定](#アクセシビリティ設定)）。 |
| `[design]` | `bold_text` | 真偽値<br>`Setting::BoldText` | `false` | すべての文字を太字にするかどうか（[アクセシビリティ設定](#アクセシビリティ設定)）。 |
| `[i18n]` | `language` | `"en" \| "ja"`<br>`Setting::Language` | 自動検出 | 表示言語。未設定時は `os::ui_language` から取得し、未対応言語の場合は `"en"` とする。 |
| `[wrist]` | `design` | 文字列（`WristDesign`）<br>`Setting::WristDesign` | `"dots"` | 腕時計パネルの文字盤デザイン。 |
| `[wrist]` | `offset` | `{ position, orientation }`<br>`Setting::WristOffset` | 未設定 (`None`) | 左手コントローラ先端から見た腕時計パネルの相対姿勢。掴み操作で調整された値が保存される。 |
| `[wrist]` | `scale` | 浮動小数点数 (`0.5`〜`3.0`)<br>`Setting::WristScale` | `1.0` | 腕時計パネルの表示倍率。 |
| `[toast]` | `anchor` | 文字列（`NotificationAnchor`）<br>`Setting::NotificationAnchor` | `"bottom"` | トースト通知の視界内固定位置（上下左右・四隅の計 8 か所）。 |
| `[dashboard]` | `distance` | 浮動小数点数 (`0.6`〜`2.0` m)<br>`Setting::DashboardDistance` | `1.3` | ダッシュボードの出現距離（0.05m 刻み）。 |
| `[dashboard]` | `height` | 浮動小数点数 (`-0.5`〜`0.5` m)<br>`Setting::DashboardHeight` | `0.0` | ダッシュボードの出現基準高さからのオフセット（0.05m 刻み）。 |
| `[settings]` | `pointer_tuning` | `"simple" \| "detailed"`<br>`Setting::PointerTuning` | `"simple"` | 設定画面におけるポインタ調整項目の表示モード。 |
| `[setup]` | `done` | 真偽値<br>`Setting::SetupDone` | `false` | 初期セットアップウィザードが完了しているかどうか。 |
| `[updater]` | `channel` | `"stable" \| "nightly"`<br>`Setting::UpdateChannel` | `"stable"` | アプリケーションの更新確認チャンネル。 |
| （設定ファイル外） | | `Setting::AutoLaunch(bool)` | ランタイム依存 | SteamVR 起動時の自動起動連携。SteamVR 自身が設定を保持する。 |

## 主な公開 API

### Kernel 型

カーネルのライフサイクルおよびシステム制御を行う主構造体。

```rust
Kernel::{
    boot,              // メディア監視・OS通知監視を起動してカーネルを初期化
    with_backend,      // テスト用バックエンドを指定した初期化
    tick,              // 1フレームの進行処理
    submit,            // 描画結果のオーバーレイ転送
    apply,             // パネルコマンドの適用
    request,           // システムへの要求の処理
    status,            // パネル状態の取得
    set,               // 設定の変更
    settings,          // 現在の設定値の取得
    os_keyboard,       // OS 仮想キーボード連携
    add_panel,         // 新規パネルの追加
    batteries,         // HMD・コントローラのバッテリー残量取得
    now_playing,       // メディア再生状態の取得
    notifications,     // 通知一覧の取得
    set_notifications, // 通知の更新
    fov,               // 両目視野範囲の取得（vr::Fov::safe 経由で HUD 配置に使用）
};
```

### パネル制御コマンド（PanelCommand）

画面側からカーネルに対してパネルの振る舞い変更を要求するコマンド群。

- `PassThrough(bool)`: コントローラレーザーの当たり判定を素通りさせる。
- `FadeOnly(bool)`: 表示・非表示の遷移演出を不透明度のフェードのみにする。
- `Hide(bool)`: パネルを破棄せず不可視化（または再可視化）する。
- `Resizable(bool)`: ユーザーによるサイズ変更の許可・不許可を設定する。
- `CornerResize(Option<ResizeLimits>)`: 角を掴んで解像度（画面の表示領域）を範囲内で変えられるようにする（既定は無効）。
- `DockSlot(Option<f32>)` / `Dockable(bool)`: ドッキングスロットの配置と、把持による分離・吸着を可能にする。
- `GrabAnchor(Option<Pose>)`: 掴み操作時の基準姿勢を指定する。
- `FocusSource`: ミラーリング対象のウィンドウを OS 側で前面化する。

### システムへの要求（Request）

どのパネルにも属さない要求。`PanelCommand` と違って対象パネルを持たず、`Kernel::request` が処理する（画面からは `ui::request` で積む）。

- `Media(media::Command)`: 再生・停止・曲送りなどのメディア制御要求を送信する。
- `Notification { id, action }`: OS 通知の本文またはボタン押下を対象アプリへ伝達する。
- `SendNotification`: デバッグ用の OS 通知を発行する。
- `RemoveNotification(id)`: 指定通知を OS 通知センターおよび内部リストから消去する。

### パネル状態（PanelStatus）

カーネルから画面側へフィードバックされる各パネルの状態情報。

- `hidden`: パネルが現在非表示状態にあるかどうか。
- `dock_hint`: ドッキング可能な領域に近づいた際の一時的な吸着ガイド情報。
- `corner_held`: 角の線（`Pointer::Corner`）のパネルで、その手が角を掴んで大きさを変えている途中か。
- `gazed`: ユーザーが視線をパネルに向けているかどうか（`Overlay::gazed`）。

## 配信しているウィンドウのメニュー

右クリックやタイトルバーのメニューは取得元とは別のウィンドウなので、配信の映像には入らない。カーネルは配信（`desktop::Stream::popups`）に出ているメニューごとにパネルを開き、配信パネルを親にして、面の同じ所に重ねる子（`Parent::over`）にする。

- **画像と出入り**: メニューの画像はカーネルが直接送る。メニューが消えたとき、配信が止まったときに閉じる。出入りのアニメーションは付けず（OS のメニュー自体に出入りの動きがある）、掴んでも動かない。
- **画像の加工**: 画像はメニューのウィンドウだけを取り込んだもので、角の外側や余白は透けている。切り抜きや影の描き足しはせず、そのまま送る。影は、アプリがメニューのウィンドウの中に描いたもの（Chrome や WinUI のメニュー）だけが出る。昔ながらのメニューの影は OS が合成のときに描き足すもので、取り込みに入らないので出ない。
- **重なりの順**: 手前のメニューほど上に重なるよう、奥のメニューから順にパネルを開く（後から開いた子ほど上になる。[`../overlay/AGENT.md`](../overlay/AGENT.md)）。
- **入力**: メニューのパネルへの入力は、配信パネルの画素に移してから配信パネルへの入力として OS へ送る。ウィンドウの外へはみ出した所は、配信パネルの面の外の座標になる。
- **UI 層との関係**: このパネルは GPUI のウィンドウを持たない。UI 層のフレームループは、カーネルが開いたパネルの PanelId を欠番として扱う。

## パネルの LOD（詳細度制御）

視界内のパネル数が増加した際の描画負荷を低減するため、距離とピクセル密度に応じた Level of Detail（LOD）制御を適用する。

- **LOD レベルの決定**: 設定ファイルの `kernel.lod_quality`（0: 無効、1: Low、2: Mid、3: High、既定値は 2）に基づき、`Kernel::panel_lod` が HMD とパネル中心点との距離を基準に算出する。距離はパネルのピクセル密度（基準: 1000px/m）で正規化されるため、引き伸ばされた粗いパネルは近傍にあるものとして扱われ、ぼやけたまま残る現象を防ぐ。
- **レーザー注視パネルの保護**: コントローラのレーザーが当たっているパネル（`Overlay::pointed`）およびツールチップは、距離に関わらず常に最高解像度（LOD0）を維持する。
- **解像度とフレームレートの間引き**:
  - 解像度: LOD0〜3 はそれぞれ等倍、1/2、1/4、1/8 の解像度へ縮小される。
  - 更新頻度: LOD0 は毎フレーム、LOD1〜3 はそれぞれ 2 / 4 / 8 フレームに 1 回のみ再描画される（`Kernel::panel_frame_due`）。描画が間引かれたフレームにおける入力イベントは、次回描画フレームへ蓄積されて処理される。
- **検証方法**: 実描画および入力の検証は `cargo test -p ui --test gpu -- --ignored panel_lod`（無効化時は `--off`）で行う。

## ロックダウン機構

初回セットアップウィザードや重要な確認画面において、ユーザーの誤操作によるパネル閉塞を防ぐ排他制御機構（`PanelCommand::Lockdown(bool)`）。

- **取得と解放**: 各パネルは任意のタイミングでロックダウンを取得できる（`PanelSpec::lockdown()` で生成時から取得可能）。すべての保持パネルが解放するか閉じるまで、システム全体のロックダウン状態（`Kernel::lockdown`）は維持される。
- **保護対象**: ロックダウンを取得しているパネルは、自身が解放するまで `PanelCommand::Close` を受信しても閉じない。ロックダウンを取得していない他パネルには直接の影響を与えない。
- **シェルの応答**: カーネルはロックダウン状態を公開するのみであり、具体的な抑制処理は各シェルが実装する。たとえば `wrist` は、ロックダウン突入時に自身とダッシュボードを非表示化し、復帰までショートカットの二回押しを無視する。
- **閉じる際の手順**: ロックダウン中のパネルを自ら閉じる場合は、コマンド列に `Lockdown(false)`、`Close` の順序で積んで送信する。

## アクセシビリティ設定

視覚補助のための 3 つの独立した設定項目。互いに干渉せず、それぞれ指定範囲内で独立して反映される。

| 設定ファイルキー | 設定列挙型 | 範囲（既定値） | 反映経路と影響範囲 |
|---|---|---|---|
| `design.text_scale` | `TextScale(f32)` | `0.8`〜`1.5` (`1.0`) | `ui::sync_settings` を経由して `design::font::set_text` へ渡され、システム全体のフォント描画サイズを一括調整する。 |
| `overlay.ui_scale` | `UiScale(f32)` | `0.7`〜`1.5` (`1.0`) | `Overlay::set_scale` に渡され、配置済みおよび新規生成される全パネルの空間寸法を一括拡大縮小する。 |
| `design.bold_text` | `BoldText(bool)` | `true` / `false` (`false`) | `text_scale` と同様にフォントシステムへ渡され、全テキストの太字ウェイトを有効化する。 |

倍率の入力値は 0.1 刻み（`SCALE_STEP`）に丸められる（不正値は等倍）。テキストスケールはレイアウト崩れを防ぐため 1.5 倍を上限とし、UI スケールは視界への収まりを考慮して 1.5 倍を上限とする。
