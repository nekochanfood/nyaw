# crates/ui

層: 4 UI

パネルを GPUI の非表示ウィンドウで描き、ポインタイベントをマウス入力へ変換する。1 つのフレームループ・カーネルで回し、描画結果を `kernel::rendering` 経由でパネルへ返す。画面（アプリ）の種類は知らない。どのパネルにも付くシステムの部品（ナビゲーションバー・仮想キーボード `ui::keyboard`・プロファイラパネル）だけは UI 層が持つ

## 主な公開 API

`run(kernel, panels, profiler)` / `run_with`（テスト用の `Hooks`）, `PanelControl::batteries` / `PanelControl::media`（毎フレーム書く）, `PanelSpec::{new, spawn, child, popover, offset, below, hinged_below, docked_to, dockable, fit_height, without_resize, titled, titled_text, without_nav_bar, with_os_keyboard, with_veil, nav_bar_spec}`, 画面をはめる場所 `PanelControl::dock`（ダッシュボードが開くと置く）と `dock` / `undock` / `docked`（はめると持ち主に `Parent { docked, slot }` で固定し、はまっていた画面は閉じる。`dockable` のパネルはナビゲーションバーの左端に外す `nav-undock` / 戻す `nav-dock` を出す。はめた画面へ打つ仮想キーボードはバーの下端に蝶番のようにつなげ、水平から手前が 10° 下がる向きに寝かせてダッシュボードに固定する（`PanelSpec::hinged_below` + `docked_to`。画面・バー・キーボードがつながって見える。画面を外すとキーボードも外れて独立する）。ほかのパネルから出すキーボードは真下に出して頭（目と手の間）へ向ける（`PanelSpec::below`））（ナビゲーションバーは既定で付き、開く側が持ち主の直後に開く。ポインタのパネルには付かない）, `pump`, `sync_panel`, `sync_settings`, `sync_tooltips`, `capture`, `open_panel_window`, `drag_preview`, `PointerBridge`, `PanelControl`, `PanelState::live`, `PanelCommand`, `PanelStatus`, `MAX_PANELS`

## ポインタの方式

ポインタの方式（`kernel::PointerMode`。カーネルが持ち、`config.toml` の `pointer_mode` が初期値、`Setting::PointerMode` で切り替え、`Settings::pointer_mode`（UI では `PanelControl::settings`）で見える。全パネル共通）:
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

- **ストレステスト（ベンチマーク）**: `crates/ui/tests/stress.rs`（`harness = false`。常には流さない: `cargo test -p ui --test stress -- --ignored`）。
  本番と同じ `ui::run_with` に novr のエミュレーター（ウィンドウは出さないが毎フレーム合成する）と GPUI 8 枚・ビームを載せ、
  マウスで掃く・押したままのスワイプ・パネルの召喚と破棄の繰り返し・目の前のパネルの局面ごとに、FPS・ドロップ率と
  レンダラー（`compositor`）・オーバーレイ（`overlay`）・UI（`input` / `draw` / `render` / `upload`）の平均と p95 を出す。
  最初と最後のアイドルの差で時間とともに重くなるか、召喚の局面の最大フレーム時間で召喚の引っ掛かりを見る。
  10 枚を重ねて出す物量の局面（`volume`）もある。
  1 フレームごとの CSV と集計は `target/tmp/stress.{csv,txt}`。`NYAW_STRESS_SCALE` で局面の長さ、
  `NYAW_STRESS_WINDOW=1` で本番と同じくウィンドウを出して GPU で合成する、`NYAW_STRESS_PROFILER=1` でプロファイラの窓も載せる。
- **長押しの輪**: `crates/ui/tests/novr_cursor.rs`（`harness = false`、`-- --ignored`）。タッチで押したまま、カーソルの輪が満ちていくのをエミュレーターの画面で見る。
- **スワイプバック（本番のフレームループ）**: `crates/ui/tests/nav_swipe.rs`（`harness = false`、`-- --ignored`）。NavTest に画面を積み、タッチのまま右から左へ動かす・長押ししてから動かすの両方で戻ることを確かめる。手ブレ抑えが効いた VR のレーザーでの確認は `testkit/tests/navigation.rs`。
- **実 GPU での読み戻し**: `crates/ui/tests/gpu_capture.rs`（`harness = false`、本番と同じプラットフォームで非表示ウィンドウを描いて読み戻す。Windows のみ）。
