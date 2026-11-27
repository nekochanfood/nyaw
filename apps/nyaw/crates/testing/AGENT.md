# crates/testing

層: 5 画面

システム開発・検証および E2E テスト用の各種検証画面を提供する。GPUI のビューとして実装され、背後の VR ランタイムの種類を意識することなく全機能の動作確認が可能である。

## 主な公開 API

各機能の単体検証用画面構造体。

```rust
Testing;             // 各種検証機能を統合したメインテストパネル
TestingAnim;         // 毎フレーム動的に変化するアニメーション検証
TestingLod;          // 距離に応じた LOD 解像度・頻度の可視化
TestingScrollbar;    // 縦・横・双方向スクロールバーの検証
StressTest;          // 最大枚数パネルの生成・破棄・配置負荷テスト
TooltipTest;         // ツールチップのオーバーフロー・追従検証
SmoothingTest;       // レーザー手振れ補正係数の動的調整検証
WizardTest;          // ウィザードコンポーネント（welcome, step, done）の検証
LogoTest;            // 各種サイズ・テーマ下におけるロゴ描画の検証
SuperellipseTest;    // スーパーエリプス角丸パラメータ（k値）の検証
ResizeTest;          // 角を掴んだサイズ変更（解像度の変更）の検証
LayerTest;           // パネルの重なり（描画順と奥行き）の検証
GuideTest;           // コントローラガイド（機種の判定と、操作部分の円・テール・説明）の検証
```

## ファイル構成と実装規則

本クレートは検証対象画面ごとに 1 ファイルを割り当てる。`panel.rs` がメインパネル（`Testing`）の描画を担当し、`lib.rs` は状態定義、カラー定数、および共通コンポーネント（ボタン、行、セクション等）のみを保持する。

| ファイル | 画面名 | 検証内容 |
|---|---|---|
| `pointer.rs` | `PointerTest` | ドラッグ＆ドロップ、スワイプ、タップ操作の検証。 |
| `nav.rs` | `NavTest` | プッシュ遷移、スワイプバック、スクロール優先度の検証。 |
| `dialog.rs` | `DialogTest` | 通常ダイアログおよびモーダルダイアログ（排他制御）の親子関係検証。 |
| `scrollbar.rs` | `TestingScrollbar` | GPUI リストおよびコンテナのスクロール挙動検証。 |
| `resize.rs` | `ResizeTest` | 角を掴んだサイズ変更（表示領域の変化と組み直し、範囲、縦横比の固定）の検証。 |
| `layers.rs` | `LayerTest` | パネルの重なりの検証。見本ごとに確定した描画順と、手前にずらした距離（mm）を表示する。 |
| `guide.rs` | `GuideTest` | 使用中のコントローラの判定（手ごと）と、[`guide`](../guide/AGENT.md) の表示。左に機種の一覧（`guide-in-use`・`guide-model-*`）と手（`guide-hand-*`）、右にガイドと割り当ての一覧（`guide-bind-<操作>`。ホバーで強調）。`guide-all-parts` で画像のすべてのボタンを囲む。 |
| `stress.rs` | `StressTest` | パネルの大量生成と破棄による負荷・スタッター検証。 |

## Testing パネルの要素一覧

メインパネル（`Testing`）上の各要素には `debug_selector` が付与されており、E2E シナリオテストから直接操作できる。

### VR 空間およびシステム連携の検証

- `click-me`: コントローラレーザーによるクリックおよびホバーの判定。
- `spawn-panel`: 隣接位置への新規検証パネルの追加生成。
- `locale-test`: 多言語表示検証画面（`LocaleTest`）の展開。言語切り替え、長文・置換文字の表示確認。
- `tooltip-test`: ツールチップ検証画面（`TooltipTest`）の展開。短文、長文（マーキー自動スクロール）、アイコンボタンのツールチップ挙動。
- `pointer-test`: 入力方式検証画面（`PointerTest`）の展開。タッチスワイプとマウスドラッグの挙動差確認。
- `dialog-test`: 子パネル検証画面（`DialogTest`）の展開。親パネルの暗転、入力遮断、および連動把持の確認。
- `settings-test`: 設定画面（[`crates/settings`](../settings/AGENT.md)）の呼び出し。
- `layer-test`: レイヤー検証画面（`LayerTest`）の展開。`layer-over`（面に重ねる子。押すたびに右下へずらして重ねる）・`layer-popover`・`layer-dialog`・`layer-hud`・`layer-far`（30cm 奥の独立パネル）で見本を出し、`layer-clear` でまとめて閉じる。各見本の `layer-readout` に確定した描画順とずらした距離が出る。
- `guide-test`: コントローラガイドの検証画面（`GuideTest`）の展開。
- `nav-test`: 画面スタック検証画面（`NavTest`）の展開。プッシュ遷移およびスワイプバックの閾値判定。
- `audio-test`: 空間音響検証画面（`AudioTest`）の展開。2ch 再生、ループ再生、立体音響の有効/無効切り替え。
- `skeleton-test`: ハンドトラッキング検証画面（`SkeletonTest`）の展開。関節ギズモの描画、ピンチジェスチャー判定。
- `notification-test`: OS 通知連携画面（`NotificationTest`）の展開。テスト通知の発行とトースト表示の確認。
- `attach-hmd` / `attach-left` / `attach-right` / `attach-tracker` / `detach`: デバイスへのアタッチおよび解除。
- `lock`: パネルの空間固定（掴み禁止）の切り替え。
- `resize-test` / `resize-test-aspect`: 角を掴んだサイズ変更の検証画面（`ResizeTest`）の展開。表示領域の大きさ（`resize-size`）と、幅に合わせて折り返すタイル（`resize-tile-N`）で、解像度が 400 × 300〜1600 × 1200 の範囲で変わり、中身が組み直されることを確認する。`resize-test-aspect` は縦横比を固定する。

### GPUI コアコンポーネントの検証

- `toggle`: クリックによるトグル状態の変更。
- `hold-me`: 押下中のアクティブスタイルの確認。
- `drag-chip` → `drop-zone`: ドラッグ＆ドロップイベントの配送。
- `open-menu` → `menu-a` / `menu-b`: ポップオーバーメニューの表示と外部クリックによる閉塞。
- `text-field`: フォーカス管理とキーボード文字入力。
- `scroll-list`: ホイールおよびドラッグによるスクロール操作。
