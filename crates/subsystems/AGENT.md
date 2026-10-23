# crates/subsystems

層: カーネル最下部 / Subsystems

OS・ランタイム・コントローラ入力・描画の差を吸収。ランタイムと入力履歴を所有し、上位には生入力のない共通フレームを渡す。設定・パネル管理・UI には依存しない

## 主な公開 API

`Subsystems::{with_backend, poll}`, `Frame`, `ControllerState`, `os`, `rendering`

## Subsystems（Issue #42）

現在の OS バックエンドは Windows のみ。対応 OS の数にかかわらずサブシステムの境界は維持する。Windows を共通仕様や既定の実装とせず、OS 固有の処理は各バックエンドに閉じる。上位層は共通 API と能力を扱い、新しい OS はバックエンドとして追加する。

このプログラムが受け入れる対象でも、OS・ランタイム・コントローラ・描画先によってその形式や能力は違う。カーネル最下部の `kernel::subsystems` がその差を吸収し、上位は共通の状態と操作だけを扱う。

```text
アプリ
├ UI（GPUI）
├ オーバーレイ（パネル管理）
└ カーネル
  ├ 設定・フレーム進行・パネル操作
  └ Subsystems（crates/subsystems）
    ├ os/         OS バックエンド → 共通のパス・設定・OS 操作
    ├ vr/         OpenVR / OpenXR / novr → 共通のランタイム操作
    ├ input/      コントローラのプロファイル → 共通アクション
    ├ rendering/  GPUI・描画転送・能力差のフォールバック
    ├ audio/      OS の音声出力・立体化
    ├ desktop/    画面・ウィンドウの取得と OS タッチ注入
    ├ media/      再生中のメディアの情報と操作
    └ keyboard/   仮想キーボードの配列データ（OS への入力は os/ の os::keyboard）
```

- 各実装クレートは `crates/subsystems/` 配下に置く。共通のライフサイクルトレイトや登録機構は設けない。
- `Subsystems<B>` がバックエンドと機種・手ごとの入力履歴を所有する。`poll()` の中で取得・機種別変換・ヒステリシスを完結し、切断や機種変更では履歴を捨てる。
- 上位へ渡す `subsystems::Frame` は `ControllerState` の配列と姿勢・トラッカー・終了要求を持つ。生入力・機種プロファイル・ランタイムの `emulated` フラグは含めない。`pose()` は追跡可能な姿勢だけを返し、`stabilize_pointer` はポインタ補正の要否を表す。
- オーバーレイは共通アクションからヒット判定・クリック・掴み・スクロールを処理する。機種別の入力名・押下しきい値・入力履歴の管理はしない。パネル操作に必要なインタラクションの状態はオーバーレイに置く。
- ランタイム指定の解釈・novr へのフォールバックは `Kernel::boot`、タイマー精度・GPU 優先度を設定する判断は `Kernel::with_backend` が行う。Subsystems は接続・OS 設定の実行手段を提供する。設定の優先順位は呼び出し側の責務。
- テストの入力注入と描画結果の確認は `kernel.subsystems.backend` を使う（旧 `kernel.vr`）。本番のフレーム取得は必ず `Subsystems::poll` を通す。既存の `kernel::vr` / `kernel::rendering` の型の再エクスポートは維持する。
- OS 差は `os`、描画ホストの差やランタイムの描画能力差は `rendering` に閉じる。新環境への対応で上位へ環境別分岐を増やさない。
- `cargo test -p subsystems -p kernel -p overlay` で機種差・左右独立の押下判定・切断と機種変更・追跡能力・パネル操作への接続を検証する。

### 共通方針の所有者

- ログのファイル出力・世代管理は `logger`、`NYAW_LOG` の取得は `kernel::config`。OS サブシステムは保存先の違いを吸収する。
- パネルを閉じた後、フェード完了で資源を解放する判断は `overlay`。`rendering::Surface` は `release` を実行し、パネルの閉鎖状態を受け取らない。
- 非表示・透過・フォーカスしないというウィンドウの方針は `ui`。描画先サイズの補正やLOD の拡大率の変え方（vendor/ の `with_render_scale`）など、実現手段の違いは描画サブシステムが吸収する。
- Subsystems とのやり取りがあるだけでは、その共通機能を Subsystems の一部にしない。同名クレートを上下に作らず、ログ・起動・パネル管理などの所有者へ置く。
