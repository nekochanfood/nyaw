# crates/kernel

層: 3 カーネル

アプリケーション全体の設定管理、フレームサイクルの進行、およびパネル操作の集約を担う。最下部の Subsystems の直上に位置し、上位の UI 層および画面クレートはカーネルを通じてのみシステムリソースを操作する。

## 設定管理システム

アプリケーションの設定は `subsystems::os::config_dir()/config.toml` に保存される。実行時の設定値はパネル操作（`PanelCommand`）から分離されており、`Kernel::set(Setting)` で更新し、`Kernel::settings()` で参照する。

設定の優先順位は「コマンドライン引数 > 設定ファイル」である（ログレベルに関しては環境変数 `NYAW_LOG` > 設定ファイル）。

### 設定項目一覧

各設定項目は `config.toml` のキー、対応する `Setting` / `Settings` の列挙型、および既定値を持つ。

#### 外観・デザイン

| 設定キー | 型 / 設定列挙型 | 既定値 | 内容 |
|---|---|---|---|
| `theme` | `"dark" \| "light"`<br>`Setting::Theme` | `"dark"` | UI の明暗テーマ。`ui::sync_settings` を通じて `design::Mode` に反映される。 |
| `[personal_color]` | `{ lightness, chroma, hue }`<br>`Setting::PersonalColor` | 未設定 (`None`) | ボタンや強調表示に使用するアクセントカラー。未設定時はテーマの既定色を用いる。 |
| `[[color_history]]` | 色配列（最大 8 件）<br>`Setting::CustomColor` | 空 | ユーザーが作成したカスタムカラーの履歴。新規作成時に先頭へ追加され、重複は除外される。 |
| `language` | `"en" \| "ja"`<br>`Setting::Language` | 自動検出 | 表示言語。未設定時は `os::ui_language` から取得し、未対応言語の場合は `"en"` とする。 |

#### 腕時計パネル (wrist)

| 設定キー | 型 / 設定列挙型 | 既定値 | 内容 |
|---|---|---|---|
| `wrist_design` | 文字列（`WristFace`）<br>`Setting::WristDesign` | `"dots"` | 腕時計パネルの文字盤デザイン。 |
| `[wrist_offset]` | `{ position, orientation }`<br>`Setting::WristOffset` | 未設定 (`None`) | 左手コントローラ先端から見た腕時計パネルの相対姿勢。掴み操作で調整された値が保存される。 |
| `wrist_scale` | 浮動小数点数 (`0.5`〜`3.0`)<br>`Setting::WristScale` | `1.0` | 腕時計パネルの表示倍率。 |

#### ポインタと入力制御

| 設定キー | 型 / 設定列挙型 | 既定値 | 内容 |
|---|---|---|---|
| `dominant_hand` | `"right" \| "left"`<br>`Setting::DominantHand` | `"right"` | 利き手の設定。デスクトップ配信パネルに対する OS マウス操作権を保持する。 |
| `pointer_smoothing` | 浮動小数点数 (`0.0`〜`0.998`)<br>`Setting::PointerSmoothing` | `0.998` | コントローラレーザーの手振れ補正係数。 |
| `pointer_release_deg` | 浮動小数点数 (`0.5`〜`45.0` 度)<br>`Setting::PointerRelease` | `11.25` | 手振れ補正中にレーザーが手から遅れた際、補正強度を半減させる閾値角度。 |
| `laser_length` | 浮動小数点数 (`0.05`〜`1.5` m)<br>`Setting::LaserLength` | `0.3` | コントローラから照射されるレーザーの可視長。 |
| `cursor_scale` | 浮動小数点数 (`0.5`〜`2.0`)<br>`Setting::CursorScale` | `1.0` | パネル上のカーソル表示倍率（0.1 刻み）。 |
| `pointer_tuning` | `"simple" \| "detailed"`<br>`Setting::PointerTuning` | `"simple"` | 設定画面におけるポインタ調整項目の表示モード。 |
| `block_input` | 真偽値<br>`Setting::BlockInput` | `true` | ダッシュボード表示中に背後のゲームに対する入力を遮断するかどうか。 |

#### 効果音・操作フィードバック

| 設定キー | 型 / 設定列挙型 | 既定値 | 内容 |
|---|---|---|---|
| `tooltips` | 真偽値<br>`Setting::Tooltips` | `true` | UI 要素に対するツールチップ表示の有効化。 |
| `tap_sound` | 真偽値<br>`Setting::TapSound` | `true` | ボタン押下時の効果音の有効化。 |
| `hover_sound` | 真偽値<br>`Setting::HoverSound` | `true` | ボタンにホバーした際の効果音の有効化。 |

#### 仮想キーボード

| 設定キー | 型 / 設定列挙型 | 既定値 | 内容 |
|---|---|---|---|
| `keyboard` | `"full" \| "tenkeyless" \| "compact"`<br>`Setting::Keyboard` | `"full"` | 仮想キーボードの配列レイアウト。 |
| `keyboard_scale` | 浮動小数点数 (`0.5`〜`2.0`)<br>`Setting::KeyboardScale` | `1.0` | 仮想キーボードの表示倍率（0.1 刻み）。 |
| `key_gap` | 浮動小数点数 (`0.0`〜`2.0`)<br>`Setting::KeyGap` | `1.0` | キー間の間隔倍率（0.1 刻み）。 |
| `key_sound` | 真偽値<br>`Setting::KeySound` | `true` | キー押下時の打鍵音の有効化。 |
| `key_sound_volume` | 浮動小数点数 (`0.0`〜`1.0`)<br>`Setting::KeySoundVolume` | `1.0` | 打鍵音の音量（0.1 刻み）。 |

#### ダッシュボードとシステム連携

| 設定キー | 型 / 設定列挙型 | 既定値 | 内容 |
|---|---|---|---|
| `dashboard_distance` | 浮動小数点数 (`0.6`〜`2.0` m)<br>`Setting::DashboardDistance` | `1.3` | ダッシュボードの出現距離（0.05m 刻み）。 |
| `dashboard_height` | 浮動小数点数 (`-0.5`〜`0.5` m)<br>`Setting::DashboardHeight` | `0.0` | ダッシュボードの出現基準高さからのオフセット（0.05m 刻み）。 |
| `dashboard_dim` | 浮動小数点数 (`0.0`〜`1.0`)<br>`Setting::DashboardDim` | `0.5` | ダッシュボード表示中に背景を暗くする度合い（黒の不透明度、0.1 刻み）。表示中の変更も即座に反映される。 |
| `mirror_frame_rate` | 整数 (`1`〜`240` FPS)<br>`Setting::MirrorFrameRate` | `60` | 画面・ウィンドウミラーリングのキャプチャ上限フレームレート。 |
| `notification_anchor` | 文字列（`NotificationAnchor`）<br>`Setting::NotificationAnchor` | `"bottom"` | トースト通知の視界内固定位置（上下左右・四隅の計 8 か所）。 |
| `update_channel` | `"stable" \| "nightly"`<br>`Setting::UpdateChannel` | `"stable"` | アプリケーションの更新確認チャンネル。 |
| （設定ファイル外） | `Setting::AutoLaunch(bool)` | ランタイム依存 | SteamVR 起動時の自動起動連携。SteamVR 自身が設定を保持する。 |
| `setup_done` | 真偽値<br>`Setting::SetupDone` | `false` | 初期セットアップウィザードが完了しているかどうか。 |

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

- `Notification { id, action }`: OS 通知の本文またはボタン押下を対象アプリへ伝達する。
- `SendNotification`: デバッグ用の OS 通知を発行する。
- `RemoveNotification(id)`: 指定通知を OS 通知センターおよび内部リストから消去する。
- `PassThrough(bool)`: コントローラレーザーの当たり判定を素通りさせる。
- `FadeOnly(bool)`: 表示・非表示の遷移演出を不透明度のフェードのみにする。
- `Hide(bool)`: パネルを破棄せず不可視化（または再可視化）する。
- `Resizable(bool)`: ユーザーによるサイズ変更の許可・不許可を設定する。
- `DockSlot(Option<f32>)` / `Dockable(bool)`: ドッキングスロットの配置と、把持による分離・吸着を可能にする。
- `GrabAnchor(Option<Pose>)`: 掴み操作時の基準姿勢を指定する。
- `Media(media::Command)`: 再生・停止・曲送りなどのメディア制御要求を送信する。
- `FocusSource`: ミラーリング対象のウィンドウを OS 側で前面化する。

### パネル状態（PanelStatus）

カーネルから画面側へフィードバックされる各パネルの状態情報。

- `hidden`: パネルが現在非表示状態にあるかどうか。
- `dock_hint`: ドッキング可能な領域に近づいた際の一時的な吸着ガイド情報。
- `gazed`: ユーザーが視線をパネルに向けているかどうか（`Overlay::gazed`）。

## パネルの LOD（詳細度制御）

視界内のパネル数が増加した際の描画負荷を低減するため、距離とピクセル密度に応じた Level of Detail（LOD）制御を適用する。

- **LOD レベルの決定**: 設定ファイル内の `lod_quality`（0: 無効、1: Low、2: Mid、3: High、既定値は 2）に基づき、`Kernel::panel_lod` が HMD とパネル中心点との距離を基準に算出する。距離はパネルのピクセル密度（基準: 1000px/m）で正規化されるため、引き伸ばされた粗いパネルは近傍にあるものとして扱われ、ぼやけたまま残る現象を防ぐ。
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
| `text_scale` | `TextScale(f32)` | `0.8`〜`1.5` (`1.0`) | `ui::sync_settings` を経由して `design::font::set_text` へ渡され、システム全体のフォント描画サイズを一括調整する。 |
| `ui_scale` | `UiScale(f32)` | `0.7`〜`1.5` (`1.0`) | `Overlay::set_scale` に渡され、配置済みおよび新規生成される全パネルの空間寸法を一括拡大縮小する。 |
| `bold_text` | `BoldText(bool)` | `true` / `false` (`false`) | `text_scale` と同様にフォントシステムへ渡され、全テキストの太字ウェイトを有効化する。 |

倍率の入力値は 0.1 刻み（`SCALE_STEP`）に丸められる（不正値は等倍）。テキストスケールはレイアウト崩れを防ぐため 1.5 倍を上限とし、UI スケールは視界への収まりを考慮して 1.5 倍を上限とする。
