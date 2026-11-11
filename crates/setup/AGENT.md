# crates/setup

層: 5 画面

セットアップウィザード。nyaw を使い始めるのに要る設定を、1 画面に 1 つずつ順に決める。ウィザード部品（`design::components` の `wizard_*` + `navigation`）で組む 1:1 のパネルで、ナビゲーションバーは付けない。

## 手順

`setup::STEPS` の順（`Step`）。どの画面も「戻る」で前へ戻れ、右から左へのスワイプでも戻る。

| 手順 | 決めること | 選び方 |
|---|---|---|
| `Language` | 言語 | 一枚目（ロゴと「ようこそ」）。言語の名前はその言語自身で出し、案内文は対応している言語すべてで出す（今の言語が先頭） |
| `Hand` | 利き手 | 左手 / 右手 |
| `Theme` | テーマ | ダーク / ライト。見本は今のパーソナルカラーで描く |
| `Color` | パーソナルカラー | `components::accent_picker`（用意した色か、スライダーで好きな色）と、その結果の見本 `components::theme_preview` |
| `Wrist` | wrist の文字盤 | 上の段で種類、下の段でその種類のデザイン（見本つき。選択肢は `kernel` の表から並ぶ） |
| `Done` | — | 「完了」で終える |

- **選ぶとその場で反映する**: 選んだものはすぐ `Setting` として積む（`PanelControl::setting_changes`）。「次へ」は進むだけで、戻っても選んだものは残る。言語を替えるとこの画面の文言もその場で替わり、テーマ・パーソナルカラーはこの画面自身が見本になる。
- **言語の初期値**: 設定ファイルで決めていなければ OS の表示言語（`../kernel/AGENT.md`）。なので、一枚目は多くの人にとって自分の言語で出る。
- **選べるカード**: 選択肢は `components::choice`（選んでいるものは塗りの色の枠と右上のチェック）。
- **文字盤の見本**: 画面どうしは直接依存しないので、main が `wrist::preview` を `FacePreview` として渡す。ロックダウン中は本物の wrist が隠れているので、見本で選ぶ。

## 出る時・終わる時

- **出る時**: 初回起動（`Settings::setup_done` が `false`）に main が `setup::spec(wrist::preview)` を載せる。デバッグからは Testing の `setup-wizard`（`testing::SetupWizardSpawn` を main が渡したときだけ出る）で `setup::spawn_spec(from, wrist::preview)` を開いて強制的にやり直せる。
- **ロックダウン**: 起動仕様が `PanelSpec::lockdown()` で開いた時点から取り、完了で放してから閉じる（`Lockdown(false)`、`Close` の順）。仕組みは `../kernel/AGENT.md`。
- **LazyFollow**: `setup::follow()`（頭から水平に 1m）で付いてくる。向きの左右だけを見て、上下・傾きは見ない。
- **完了**: `Setting::SetupDone(true)` を設定ファイルに残す。途中でやめても、選んだ設定は残る（次の起動でまたウィザードが出る）。

## 主な公開 API

`setup::spec(faces)`（初回起動）, `setup::spawn_spec(from, faces)`（デバッグからのやり直し）, `setup::follow()`, `SetupWizard`（`step()`）, `Step`, `STEPS`, `FacePreview`

## テスト

- 通しの操作: `crates/testkit/tests/scenarios/setup.rs`（各画面で選ぶと設定が変わり、完了で終わる）、`setup_lockdown.rs`（ロックダウンと追従）。
- 見た目: `cargo test -p ui --test gpu -- --ignored look`（各画面を `target/tmp/look-setup-*.bmp` に書き出す。ライトのものは日本語）。
