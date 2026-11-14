# crates/subsystems/keyboard

層: 1 Subsystems

仮想キーボードの配列データ（xkeyboard-config）・文字・トグル状態。OS には触らない（OS への入力は `os::keyboard`）

## 主な公開 API

`Database`, `Keyboard`, `Layout`, `Stroke`, `KeyboardType`

## 仮想キーボード

キーボードは役割ごとに 3 か所に分かれる。

- `crates/subsystems/keyboard`: 配列データ（xkeyboard-config）と、押したキーから文字・修飾キーの状態を求める（`Stroke`）。OS には触らない。`Database::load` は数百のファイルをディスクから読む（初回は書き出しと比較も）ので、ディスクが混んでいると数秒掛かる。UI はプロセスに 1 つだけ、起動時に裏のスレッドで読んでおく（下の `ui::keyboard`）。
- `crates/subsystems/os` の `os::keyboard`: OS の前面のアプリの配列・IME の監視（`Watcher`。別スレッド）と、キー・文字の注入。カーネルが持ち（`Kernel::os_keyboard`。最初に使ったときに監視を始める）、`PanelCommand::TypeToOs` で送る。
- `crates/ui` の `ui::keyboard`: キーボードのパネル（`KeyboardView`）と、入力をどのパネルへ届けるかの振り分け（`InputRouting`）。GPUI の入力欄は `focus_input`、OS を操作するパネル（配信・`PanelSpec::with_os_keyboard`）は `toggle_os_keyboard` で出す。OS の状態はフレームループが `PanelControl::os_keyboard` に書き写し、`system_keyboard` で読む。配列のデータベースは `ui::run_with` の起動時に読み始め（`keyboard::preload`）、どの `KeyboardView` もそれを共有して、配列を GPUI の裏のスレッドで組み、組めたらすぐ載せる（初めてキーボードを出すときに読み始めていたので、読み終わるまでキーの無い白いカードが出ていた）。
