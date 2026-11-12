# crates/ui

層: 4 UI

パネルを GPUI の非表示ウィンドウで描き、ポインタイベントをマウス入力へ変換する。1 つのフレームループ・カーネルで回し、描画結果を `kernel::rendering` 経由でパネルへ返す。画面（アプリ）の種類は知らない。どのパネルにも付くシステムの部品（ナビゲーションバー・仮想キーボード `ui::keyboard`・プロファイラパネル）と、設定画面とセットアップウィザードが共有する設定の部品（`ui::prefs`。下の「設定の部品」）だけは UI 層が持つ

## 主な公開 API

`run(kernel, panels, profiler)` / `run_with`（テスト用の `Hooks`）, `PanelControl::batteries` / `PanelControl::media`（毎フレーム書く）, `PanelSpec::{new, spawn, child, popover, offset, typing, docked_to, dockable, fit_height, without_resize, pointer_mode, titled, titled_text, without_nav_bar, with_os_keyboard, with_veil, nav_bar_spec}`, 画面をはめる場所 `PanelControl::dock`（ダッシュボードが開くと置く）と `dock` / `undock` / `docked`（はめると持ち主に `Parent { docked, slot }` で固定し、はまっていた画面は閉じる。`dockable` のパネルはナビゲーションバーの左端に外す `nav-undock` / 戻す `nav-dock` を出す。仮想キーボードは、はめた画面へ打つものも含め、対象パネルの下ではなく開いた瞬間の頭の前の打ちやすい位置に出す。頭（と手）から決めるのは高さと仰ぐ角度だけで、左右の向きと位置は呼び出し元に合わせる（呼び出し元の面と同じ向きで、ホームポジション（F と J の間）を呼び出し元の中心の左右に合わせる。頭の回転に付いて行かない）。立っている間の高さはコントローラ（両手なら間）の少し上（手を動かさず手首を起こすだけで指せる。上端は目線より下、下は見える所まで）、面は目を重く見つつ手の方へも少し向ける（コントローラが無ければ目と手の間へ）。寝ている間は手に合わせず、視界に入る頭の正面の下寄り（正面のパネルに重ならない所）に出して目へ向ける（傾きに応じて滑らかに移る）。どちらでも、頭から見て呼び出し元（ナビゲーションバー・はめた先のダッシュボードのバーも）の下端より少し下に見えるよう、必要な分だけ下げる（呼び出し元に重ねない）。掴むとホームポジションを中心に動かして頭へ向ける（`PanelSpec::grab_anchor`。大きさを変えると基準も付いて動く）。親子関係は作らず、呼び出し元を覚えておき、呼び出し元が閉じる・隠れるとキーボードも閉じる（`PanelSpec::typing`、`keyboard::panel_closed` / `panel_hidden`））（ナビゲーションバーは既定で付き、開く側が持ち主の直後に開く。ポインタのパネルには付かない）, `pump`, `sync_panel`, `sync_settings`, `sync_tooltips`, `capture`, `open_panel_window`, `drag_preview`, `PointerBridge`, `PanelControl`, `PanelState::live`, `PanelCommand`, `PanelStatus`, `MAX_PANELS`

## 設定の部品（`ui::prefs`）

全体の設定（`Setting`）を選ぶ画面の一部。設定画面とセットアップウィザードの両方が同じものを出す（画面どうしは直接依存しないので、共有する部品は UI 層に置く）。今の設定（`PanelControl::settings`）を見せ、選ぶとすぐ `PanelControl::setting_changes` に積む。地（`background`）の上に置く前提。開閉などの小さな状態（`Appearance` / `WatchFaces`。どちらも `Copy`）は呼び出し側が持ち、変えるときは `set` で受け取る（ビューの型を問わない）。

- **`prefs::appearance(panel, state, set, window, cx)`**: 外観。上からテーマの見本（`theme-preview`）、テーマ（`setting-theme`）、パーソナルカラー（履歴の見本 `accent-<名前>` を左右中央に並べ、右端のプラス `accent-custom`）。プラスを押すと、色を作るダイアログ `prefs::ColorDialog` を `panel` の子として開く（開いている間、元の画面は暗くなって入力を受けない）。ダイアログはスライダーと見本を出し、`color-ok` で `Setting::CustomColor`（パーソナルカラーにして履歴の先頭に覚える）、`color-cancel` か外側を押すと何も変えずに閉じる。決めるまで、ほかの画面の色は変えない。
- **`prefs::watch_faces(state, set, window, cx)`**: wrist の文字盤。上から種類の絞り込み（`watch-kind`: すべて / スタンダード / デジタル / アナログ）、弧に並ぶ見本（`watch-faces`。`design::components::carousel` + `arc`。押したまま左右に動かすか、左右のキャレット `watch-prev` / `watch-next` で回す。端はつながる）、中央のデザインの名前、`watch-set`（押すとそのデザインにする。使っているデザインでは押せない見た目で「設定中」）。回すだけでは設定は変わらない。使っているデザインの見本は、塗りの色の輪で囲む。
- **文字盤の見本**: 描き方は wrist が GPUI のグローバル `prefs::FacePreview` に置く（`Wrist::new`）。置かれていなければ地の丸だけになる（wrist を載せないテスト）。
- テスト: `prefs::tests`、操作は `crates/testkit/tests/scenarios/prefs.rs`、見た目は `look`（`look-*.bmp` / `look-watch-*.bmp` / `look-color-*.bmp`）。

## ポインタの方式

ポインタの方式（`kernel::PointerMode`。カーネルが持ち、`config.toml` の `pointer_mode` が初期値、`Setting::PointerMode` で切り替え、`Settings::pointer_mode`（UI では `PanelControl::settings`）で見える。全パネル共通。ただしパネルごとに `PanelSpec::pointer_mode`（`PanelCommand::PointerMode`、`Kernel::panel_pointer_mode`）で決められ、仮想キーボードは押した瞬間に押し、手ブレでスワイプにならないようマウス。キーは見た目ではなく隙間まで含めた升目（`Keycap::regions`）で押す）:
- タッチ（既定）: 押したまま頭から見て `ui::SWIPE_SLOP_DEG`（1°。パネルの距離と密度から毎フレーム px に直す。下限 `ui::SWIPE_SLOP` = 12px）より動かすとスワイプ（GPUI にはピクセル単位の `ScrollWheel`、宛先は押した所。
  離すと慣性で滑る: `PointerBridge::momentum` をフレームごとに流す）、動かさずに離すとタップ
  （その時点で押して離す）、動かさずに `ui::LONG_PRESS`（400ms）押し続けるとマウスの押下（そのまま動かせば `on_drag` のドラッグ＆ドロップ）。
  押下はどれと決まるまで送らないので、押している間の見た目は長押しまで効かない。方式は押した瞬間に決まり、離すまで変わらない。
- マウス: 押したまま動かすとドラッグ。
- どちらの方式でも、トリガーを押している間は押したパネルの面を指し続ける（オーバーレイ層のポインタのキャプチャ）。
  スライド・ドラッグがパネルからはみ出しても離すまで続き、離したフレームの Up もそのパネルに届く。
- testkit のキット は操作のシナリオをマウスで書く（設定でマウスを明示）。タッチは `pointer_mode.rs` で確かめる。

未対応: GPUI 標準の `.tooltip()`（遅延タイマー。代わりに `design` の `Button::tooltip` / `components::hover_tooltip` で置き、`ui::sync_tooltips` が手に振り分けて beam が描く）、右クリック・ダブルクリック（レーザーは左ボタン1回押しのみ）、文字・画像の見た目（testkit のラスタライザは矩形・角丸・枠線・下線を描く）。

## テスト・ベンチマーク

- **見た目の書き出し**: `crates/ui/tests/gpu/look.rs`（`cargo test -p ui --test gpu -- --ignored look`）。設定画面の「外観」「時計」（`look-watch-*.bmp`）・色を作るダイアログ（`look-color-*.bmp`）・wrist の文字盤の見本（全デザイン。`look-faces-*.bmp`）・セットアップウィザードの各画面（`look-setup-*.bmp`。ライトのものは日本語）をテーマとパーソナルカラーを替えながら実 GPU で描き、`target/tmp/look-*.bmp` に書き出す（塗りが選んだ色で描かれていることだけ検査する）。
- **ストレステスト（ベンチマーク）**: `crates/ui/tests/gpu/stress.rs`（`harness = false`。常には流さない: `cargo test -p ui --test gpu -- --ignored stress`）。
  本番と同じ `ui::run_with` に novr のエミュレーター（ウィンドウは出さないが毎フレーム合成する）と GPUI 8 枚・ビームを載せ、
  マウスで掃く・押したままのスワイプ・パネルの召喚と破棄の繰り返し・目の前のパネルの局面ごとに、FPS・ドロップ率と
  レンダラー（`compositor`）・オーバーレイ（`overlay`）・UI（`input` / `draw` / `render` / `upload`）の平均と p95 を出す。
  最初と最後のアイドルの差で時間とともに重くなるか、召喚の局面の最大フレーム時間で召喚の引っ掛かりを見る。
  10 枚を重ねて出す物量の局面（`volume`）もある。
  1 フレームごとの CSV と集計は `target/tmp/stress.{csv,txt}`。`NYAW_STRESS_SCALE` で局面の長さ、
  `NYAW_STRESS_WINDOW=1` で本番と同じくウィンドウを出して GPU で合成する、`NYAW_STRESS_PROFILER=1` でプロファイラの窓も載せる。
- **長押しの輪**: `crates/ui/tests/gpu/novr_cursor.rs`（`harness = false`、`-- --ignored`）。タッチで押したまま、カーソルの輪が満ちていくのをエミュレーターの画面で見る。
- **novr のキー入力**: `crates/ui/tests/gpu/novr_keys.rs`（`harness = false`、`-- --ignored`。Windows のみ）。本番と同じ窓付きのエミュレーターに X キーの押下を投げ、二回押しがすべてカーネルに届くことを確かめる（GPUI のメッセージループが、同じスレッドにいるエミュレーターの窓宛ての `WM_KEYDOWN` を捨てていた。`vendor/gpui_windows` の `translate_accelerator`）。
- **スワイプバック（本番のフレームループ）**: `crates/ui/tests/gpu/nav_swipe.rs`（`harness = false`、`-- --ignored`）。NavTest に画面を積み、タッチのまま右から左へ動かす・長押ししてから動かすの両方で戻ることを確かめる。手ブレ抑えが効いた VR のレーザーでの確認は `testkit/tests/scenarios/navigation.rs`。
- **実 GPU での読み戻し**: `crates/ui/tests/gpu/gpu_capture.rs`（`harness = false`、本番と同じプラットフォームで非表示ウィンドウを描いて読み戻す。Windows のみ）。
- **ゆるい追従**: `PanelSpec::lazy_follow(kernel::LazyFollow)` で、パネルを頭の向き（左右だけ）と歩きにゆるく追従させる（どのパネルでも使える。仕組みは `../overlay/AGENT.md`）。
- **ロックダウン**: `PanelSpec::lockdown()` で、開いた時点から `PanelCommand::Lockdown` を取る（放すのはビュー。仕組みは `../kernel/AGENT.md`）。
