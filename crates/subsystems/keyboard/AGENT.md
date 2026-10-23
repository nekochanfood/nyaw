# crates/subsystems/keyboard

層: カーネル / Subsystems / キーボード

仮想キーボードの配列データ（xkeyboard-config）・文字・トグル状態。OS には触らない（OS への入力は `os::keyboard`）

## 主な公開 API

`Database`, `Keyboard`, `Layout`, `Stroke`, `KeyboardType`

## 仮想キーボード

キーボードは役割ごとに 3 か所に分かれる。

- `crates/subsystems/keyboard`: 配列データ（xkeyboard-config）と、押したキーから文字・修飾キーの状態を求める（`Stroke`）。OS には触らない。
- `crates/subsystems/os` の `os::keyboard`: OS の前面のアプリの配列・IME の監視（`Watcher`。別スレッド）と、キー・文字の注入。カーネルが持ち（`Kernel::os_keyboard`。最初に使ったときに監視を始める）、`PanelCommand::TypeToOs` で送る。
- `crates/ui` の `ui::keyboard`: キーボードのパネル（`KeyboardView`）と、入力をどのパネルへ届けるかの振り分け（`InputRouting`）。GPUI の入力欄は `focus_input`、OS を操作するパネル（配信・`PanelSpec::with_os_keyboard`）は `toggle_os_keyboard` で出す。OS の状態はフレームループが `PanelControl::os_keyboard` に書き写し、`system_keyboard` で読む。
