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
```

## ファイル構成と実装規則

本クレートは検証対象画面ごとに 1 ファイルを割り当てる。`panel.rs` がメインパネル（`Testing`）の描画を担当し、`lib.rs` は状態定義、カラー定数、および共通コンポーネント（ボタン、行、セクション等）のみを保持する。

| ファイル | 画面名 | 検証内容 |
|---|---|---|
| `pointer.rs` | `PointerTest` | ドラッグ＆ドロップ、スワイプ、タップ操作の検証。 |
| `nav.rs` | `NavTest` | プッシュ遷移、スワイプバック、スクロール優先度の検証。 |
| `dialog.rs` | `DialogTest` | 通常ダイアログおよびモーダルダイアログ（排他制御）の親子関係検証。 |
| `scrollbar.rs` | `TestingScrollbar` | GPUI リストおよびコンテナのスクロール挙動検証。 |
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
- `nav-test`: 画面スタック検証画面（`NavTest`）の展開。プッシュ遷移およびスワイプバックの閾値判定。
- `audio-test`: 空間音響検証画面（`AudioTest`）の展開。2ch 再生、ループ再生、立体音響の有効/無効切り替え。
- `skeleton-test`: ハンドトラッキング検証画面（`SkeletonTest`）の展開。関節ギズモの描画、ピンチジェスチャー判定。
- `notification-test`: OS 通知連携画面（`NotificationTest`）の展開。テスト通知の発行とトースト表示の確認。
- `attach-hmd` / `attach-left` / `attach-right` / `attach-tracker` / `detach`: デバイスへのアタッチおよび解除。
- `lock`: パネルの空間固定（掴み禁止）の切り替え。

### GPUI コアコンポーネントの検証

- `toggle`: クリックによるトグル状態の変更。
- `hold-me`: 押下中のアクティブスタイルの確認。
- `drag-chip` → `drop-zone`: ドラッグ＆ドロップイベントの配送。
- `open-menu` → `menu-a` / `menu-b`: ポップオーバーメニューの表示と外部クリックによる閉塞。
- `text-field`: フォーカス管理とキーボード文字入力。
- `scroll-list`: ホイールおよびドラッグによるスクロール操作。
