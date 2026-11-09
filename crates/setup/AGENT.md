# crates/setup

層: 5 画面

セットアップウィザード。ウィザード部品（`design::components` の `wizard_*` + `navigation`）で 3 枚: 一枚目は nyaw のフルロゴのようこそ、二枚目は仮置きの画面（セットアップ用の画面ができたらここへ）、三枚目は「You're all set!」の完了。1:1 のパネルで、ナビゲーションバーは付けない。

- **出る時**: 初回起動（`Settings::setup_done` が `false`）に main が `setup::spec()` を載せる。デバッグからは Testing の `setup-wizard`（`testing::SetupWizardSpawn` を main が渡したときだけ出る）で `setup::spawn_spec(from)` を開いて強制的にやり直せる。画面どうしは直接依存しないので、Testing は main が渡す関数で開く。
- **ロックダウン**: 起動仕様が `PanelSpec::lockdown()` で開いた時点から取り、完了で放してから閉じる（`Lockdown(false)`、`Close` の順）。仕組みは `../kernel/AGENT.md`。
- **LazyFollow**: `setup::follow()`（頭から水平に 1m）で付いてくる。向きの左右だけを見て、上下・傾きは見ない。
- **完了**: `Setting::SetupDone(true)` を設定ファイルに残す。

## 主な公開 API

`setup::spec()`（初回起動）, `setup::spawn_spec(from)`（デバッグからのやり直し）, `setup::follow()`, `SetupWizard`, `STEPS`
