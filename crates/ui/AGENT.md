# crates/ui

層: 4 UI

パネルを GPUI の非表示ウィンドウで描き、ポインタイベントをマウス入力へ変換する。1 つのフレームループ・カーネルで回し、描画結果を `kernel::rendering` 経由でパネルへ返す。画面（アプリ）の種類は知らない。どのパネルにも付くシステムの部品（ナビゲーションバー・仮想キーボード `ui::keyboard`・プロファイラパネル）と、設定画面とセットアップウィザードが共有する設定の部品（`ui::prefs`。下の「設定の部品」）だけは UI 層が持つ

## 主な公開 API

`run(kernel, panels, profiler)` / `run_with`（テスト用の `Hooks`）, `PanelControl::batteries` / `PanelControl::media`（毎フレーム書く）, `PanelSpec::{new, spawn, child, popover, offset, typing, docked_to, dockable, fit_height, without_resize, reach, pointer_mode, titled, titled_text, without_nav_bar, with_os_keyboard, with_veil, nav_bar_spec, size}`, 画面をはめる場所 `PanelControl::dock`（ダッシュボードが開くと置く）と `dock` / `undock` / `docked`（はめると持ち主に `Parent { docked, slot }` で固定し、はまっていた画面は閉じる。`dockable` のパネルはナビゲーションバーの左端に外す `nav-undock` / 戻す `nav-dock` を出す。仮想キーボードは、はめた画面へ打つものも含め、対象パネルの下ではなく開いた瞬間の頭の前の打ちやすい位置に出す。頭（と手）から決めるのは高さと仰ぐ角度だけで、左右の向きと位置は呼び出し元に合わせる（呼び出し元の面と同じ向きで、ホームポジション（F と J の間）を呼び出し元の中心の左右に合わせる。頭の回転に付いて行かない）。立っている間の高さはコントローラ（両手なら間）の少し上（手を動かさず手首を起こすだけで指せる。上端は目線より下、下は見える所まで）、面は目を重く見つつ手の方へも少し向ける（コントローラが無ければ目と手の間へ）。寝ている間は手に合わせず、視界に入る頭の正面の下寄り（正面のパネルに重ならない所）に出して目へ向ける（傾きに応じて滑らかに移る）。どちらでも、頭から見て呼び出し元（ナビゲーションバー・はめた先のダッシュボードのバーも）の下端より少し下に見えるよう、必要な分だけ下げる（呼び出し元に重ねない）。掴むとホームポジションを中心に動かして頭へ向ける（`PanelSpec::grab_anchor`。大きさを変えると基準も付いて動く）。大きさは種類ごとの標準で、ホームポジションに向けた手首を手の甲の方（外）へ 22.5° 倒すと遠い方のキーの端に届く幅（手からキーボードまで 45cm とみる。`keyboard::WRIST_TILT` / `HAND_DISTANCE`。種類を替えると出ているものもその種類の標準の大きさになる）。親子関係は作らず、呼び出し元を覚えておき、呼び出し元が閉じる・隠れるとキーボードも閉じる（`PanelSpec::typing`、`keyboard::panel_closed` / `panel_hidden`））（ナビゲーションバーは既定で付き、開く側が持ち主の直後に開く。ポインタのパネルには付かない）, `pump`, `sync_panel`, `sync_settings`, `sync_tooltips`, `capture`, `open_panel_window`, `drag_preview`, `PointerBridge`, `PanelControl`, `PanelState::live`, `PanelCommand`, `PanelStatus`, `MAX_PANELS`, 画面からの依頼 `command` / `commands` / `set` / `settings` / `notify` / `redraw`（下の「画面からの依頼」）

## ファイルの分け方（ルール）

**役割ごとに 1 ファイル**。`lib.rs` はモジュールの宣言・再エクスポートと、いくつものファイルが使う定数だけを持ち、中身を書かない。1 つのファイルだけが使う定数はそのファイルに置く。テストはそのファイルの子（`pointer/tests.rs` など。非公開の項目に触れる）に置き、共通の道具は `test_util.rs`。

| ファイル | 中身 |
|---|---|
| `control.rs` | ビューとフレームループの受け渡し口（`PanelControl` / `PanelState`）と、画面が使う操作（`command(s)` / `set` / `settings` / `dock` / `notify` など）、設定の反映（`sync_settings` / `lang` / `language`） |
| `spec.rs` | 起動仕様（`PanelSpec`）と、それからパネルを開く `open_panel` |
| `nav_bar.rs` | ナビゲーションバー（`NavBar`）と、中身を包む根（`PanelRoot`） |
| `pointer.rs` | レーザーのイベントを GPUI の入力に変える（`PointerBridge`。タップ・スワイプ・長押し）と `dispatch_input` |
| `drag.rs` | ドラッグの見た目（`DragPreview`） |
| `run.rs` | フレームループ（`run` / `pump` / ツールチップ） |
| `profiler_panel.rs` | プロファイラのパネル |
| `keyboard` / `prefs` / `mirror` / `link` / `cue` / `panel_window` | 前からの部品（下の各節） |

## 画面からの依頼

画面は `PanelControl` の中身を自分で組み立てず、次の入口で頼む（フレームループが次のフレームで反映する）。画面ごとに同じ補助関数を書かない。

- `ui::command(panel, PanelCommand, cx)` / `ui::commands(panel, [..], cx)`: パネルへの操作（積んだ順に反映）。
- `ui::set(Setting, cx)` / `ui::settings(cx)`: 全体の設定の変更と、今の設定（無ければ既定）。
- URL をブラウザで開くのは `design::link::open(url, window, cx)`（下の「リンクを開く」）。
- `ui::notify(panel, cx)`: タイマーや非同期の結果で自分から変わったビューの `cx.notify()`。フレームループはビューの notify を知らないので、素の `cx.notify()` だけでは次の推測の描き直し（最長 1 秒）まで絵が変わらない。ビューの外（`&mut App`）からは `ui::redraw(panel, cx)`。

## リンクを開く（検問所）

nyaw の中から URL を開く道は 1 本に絞り、検問所 `ui::link` を通す。公式のページ（リリースページ・リポジトリ）でも必ず確かめる。

- 入口は `design::link::open(url, window, cx)` だけ。部品（Markdown のリンク）も画面（更新・ライセンス・ソースコード）もこれで依頼する。依頼はウィンドウごとに積まれる。
- `dispatch_input` が入力を配送した直後に `link::flush` で取り出し、依頼元のパネルの子としてダイアログ（`link-dialog`）を開く。タイトルは「このリンクをブラウザで開いてもいいですか？」、概要に URL を省かずそのまま出す。`link-open` で開いて閉じ、`link-cancel` か外側を押すと開かずに閉じる。
- `http` / `https` 以外（`file:`・`ms-settings:` など、OS がファイルやアプリを開くもの）はダイアログも出さずに捨てる（警告のログ）。
- `App::open_url` を呼んでよいのは検問所の出口 `link::open` だけ。ほかで呼ぶとルートの `clippy.toml`（`disallowed-methods`）で CI の clippy が落ちる。
- テスト: `link::tests`（依頼はダイアログになりブラウザは開かない・ブラウザで開くものだけ通す・ダイアログは見せた URL だけを開く）、`design/tests/markdown.rs`（リンクを押すと依頼になる）。

## 設定の部品（`ui::prefs`）

全体の設定（`Setting`）を選ぶ画面の一部。設定画面とセットアップウィザードの両方が同じものを出す（画面どうしは直接依存しないので、共有する部品は UI 層に置く）。今の設定（`PanelControl::settings`）を見せ、選ぶとすぐ `PanelControl::setting_changes` に積む。地（`background`）の上に置く前提。開閉などの小さな状態（`Appearance` / `WatchFaces`。どちらも `Copy`）は呼び出し側が持ち、変えるときは `set` で受け取る（ビューの型を問わない）。

- **`prefs::appearance(panel, state, set, window, cx)`**: 外観。上からテーマの見本（`theme-preview`）、テーマ（`setting-theme`）、パーソナルカラー（履歴の見本 `accent-<名前>` を左右中央に並べ、右端のプラス `accent-custom`）。プラスを押すと、色を作るダイアログ `prefs::ColorDialog` を `panel` の子として開く（開いている間、元の画面は暗くなって入力を受けない）。ダイアログはスライダーと見本を出し、`color-ok` で `Setting::CustomColor`（パーソナルカラーにして履歴の先頭に覚える）、`color-cancel` か外側を押すと何も変えずに閉じる。決めるまで、ほかの画面の色は変えない。
- **`prefs::watch_faces(state, set, window, cx)`**: wrist の文字盤。上から種類の絞り込み（`watch-kind`: すべて / スタンダード / デジタル / アナログ）、弧に並ぶ見本（`watch-faces`。`design::components::carousel` + `arc`。押したまま左右に動かすか、左右のキャレット `watch-prev` / `watch-next` で回す。端はつながる）、中央のデザインの名前、`watch-set`（押すとそのデザインにする。使っているデザインでは押せない見た目で「設定中」）。回すだけでは設定は変わらない。使っているデザインの見本は、塗りの色の輪で囲む。
- **`prefs::accessibility(window, cx)`**: アクセシビリティ。上に文字の見本（高さの決まった箱 `a11y-preview` の中に `a11y-sample`: アイコンの付いた見出し・本文・補足）、下に対等な 3 つの行: 文字の大きさ（`setting-text-size`。スライダー `setting-text-size-slider`）、UI スケール（`setting-ui-scale`。`setting-ui-scale-slider`）、太字（`setting-bold-text`。スイッチ）。倍率は 10% の刻みに吸い付き、今の値を行に % で出す。文字の大きさは動かしている間もすぐ効く（見本の箱の高さは決まっていて、スライダーは行の右端にあるので、文字が変わっても手元のスライダーは動かない）。UI スケールは決めるとパネルそのものが大きくなってレーザーの下で動いてしまうので、動かしている間は見本だけを決めた後の大きさに拡大・縮小して見せ、離したときに決める（`Slider::on_release`）。動かしている途中の値はこの部品が持つので、呼び出し側に状態は要らない。
- **`prefs::keyboard_preview(&keys, cx)`**: キーボードの見本。高さの決まった箱 `keyboard-preview` の中央に、今の設定の種類・OS の配列の仮想キーボード（`virtual-keyboard`）を、召喚したキーボードと同じ地（カードと四方の余白。`keyboard-preview-card`。閉じるボタンとその段は無い）に載せて見せる。縮尺は決め打ちにせず、置かれた場所を測って収まるいちばん大きい倍率（実寸まで）にする（`keyboard::preview(&keys, cx)`。箱に限らず、渡された場所に合わせる。地の余白と角丸も同じ倍率）。種類を替えても、画面の幅が変わっても、箱と下の行の幅からはみ出さない（設定画面では、フルサイズは幅いっぱい、ほかの種類は高さいっぱい）。箱の高さは決まっているので、種類を替えても下の項目は動かない。キーは押せて、キーの音の設定（`key_sound` / `key_sound_volume`）どおりに鳴るが、打った文字はどこにも送らないダミー（`KeyboardView::preview`: `preview` = 送らない・繰り返さない・OS の配列が取れていなくても押せる。`unit` = キー 1u の大きさは `keyboard::preview` が毎回決める。隙間・角丸・文字も比例する）。見せるキーボードは `prefs::new_keyboard_preview(panel, cx)` で作って呼び出し側が持つ（配列は裏のスレッドで読むので、作り直さず持ち続ける）。下に並べる項目（種類 `setting-keyboard`・キーの入力音 `setting-key-sound`・その音量 `setting-key-volume`）は画面の側が置く。
- **文字盤の見本**: 描き方は wrist が GPUI のグローバル `prefs::FacePreview` に置く（`Wrist::new`）。置かれていなければ地の丸だけになる（wrist を載せないテスト）。
- テスト: `prefs::tests`、操作は `crates/testkit/tests/scenarios/prefs.rs`（キーボードの見本の音の大きさは `crates/testkit/tests/audio.rs`）、見た目は `look`（`look-*.bmp` / `look-watch-*.bmp` / `look-keyboard-*.bmp` / `look-color-*.bmp`）。

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

未対応: GPUI 標準の `.tooltip()`（遅延タイマー。代わりに `design` の `Button::tooltip` / `components::hover_tooltip` で置き、`ui::sync_tooltips` が手に振り分けて beam が描く。全体の設定 `tooltips` がオフなら振り分けない）、右クリック・ダブルクリック（レーザーは左ボタン1回押しのみ）、文字・画像の見た目（testkit のラスタライザは矩形・角丸・枠線・下線を描く）。

## テスト・ベンチマーク

- **見た目の書き出し**: `crates/ui/tests/gpu/look.rs`（`cargo test -p ui --test gpu -- --ignored look`）。設定画面の「外観」「時計」（`look-watch-*.bmp`）「キーボード」（`look-keyboard-*.bmp`）・色を作るダイアログ（`look-color-*.bmp`）・wrist の文字盤の見本（全デザイン。`look-faces-*.bmp`）・セットアップウィザードの各画面（`look-setup-*.bmp`。ライトのものは日本語）をテーマとパーソナルカラーを替えながら実 GPU で描き、`target/tmp/look-*.bmp` に書き出す（塗りが選んだ色で描かれていることだけ検査する）。設定画面は本番と同じに `PanelRoot` に載せる（載せないとパネルの影の余白が付かず、中身が本番より広く描かれる）。
- **アクセシビリティの見た目**: `crates/ui/tests/gpu/a11y.rs`（`cargo test -p ui --test gpu -- --ignored a11y`）。文字の大きさ（80% / 100% / 150%）と太字化を替えながら、設定画面の各カテゴリ・色を作るダイアログ・文字盤の見本・仮想キーボード・Testing・ダッシュボード（バー・クイック設定・ランチャー）を実 GPU で描き、`target/tmp/look-a11y-<状態>-<画面>.bmp` に書き出す。検査するのは、倍率・太字化で文字のインクが増えること（等幅も）、`.text_exact()` の文字盤が 1 画素も変わらないこと。操作は `crates/testkit/tests/scenarios/prefs.rs`（`accessibility_*` / `ui_scale_*`）。
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
