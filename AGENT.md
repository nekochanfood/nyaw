# AGENT.md

nyaw は GPUI で GUI を描く VR オーバーレイシステム。Cargo ワークスペースで、層ごとに 1 クレート。

## 層構成（下 → 上）

| 層 | クレート | 責務 | 主な公開 API |
|---|---|---|---|
| - | `crates/logger` | logfmt 形式の構造化ロガー（横断）。整形・レベル・出力先 `Sink` の抽象と、`log` クレート（gpui 等）からの橋渡し | `trace!/debug!/info!/warn!/error!`, `Logger`, `Sink`, `StderrSink`, `init`, `format` |
| 0 OS | `crates/os` | Windows/Linux の差を吸収（パス、環境変数、ランタイム検出、ファイル入出力）。`#[cfg]` はここだけに書く | `config_dir`, `log_dir`, `log_level`, `FileSink`, `openvr_paths_file`, `openxr_runtime_manifest` |
yaw`、Linux `$XDG_STATE_HOME/nyaw`）を読む。無ければ既定値、知らないキーは無視。キーは `log`（ログレベル）、`runtime`（`openvr`/`openxr`/`novr`）、`pointer_smoothing`（レーザーの手ブレ抑え）、`pointer_mode`（`mouse`/`touch`。レーザーのエミュレート方式、既定 `mouse`）。優先順位はコマンドライン > 設定ファイル（ログレベルは `NYAW_LOG` > 設定ファイル）で、決めるのは `main` | `load`, `parse`, `path`, `Config` |
| 2 ランタイム | `crates/vr` | OpenVR / OpenXR / novr の差を吸収。上位層は `Box<dyn VrBackend>` しか持たない | `connect`, `VrBackend`, `Runtime`, `Surface`, `Pose`, `Frame`（`pose(&Device)`）, `Device`, `RawController`, `mock::MockBackend`（`test-support` フィーチャー時のみ） |
| 3 入力 | `crates/input` | コントローラ機種の差を吸収（ドライバ的役割）。プロファイル表で生入力 → 抽象アクション | `map(&RawController) -> ControllerState`, `source(profile, Action)` / `scroll_source(profile)`（逆引き） |
| 4 オーバーレイ | `crates/overlay` | パネル配置、デバイスへの取り付け（追従）・掴めないようにするロック（空間固定）、レーザーのヒット判定、ポインタイベント生成、パネルの掴み移動 | `Overlay::{add_panel, attach, detach, set_locked, set_resolution, tick, submit}`, `Panel::pixel_to_world`, `UiEvent`, `PointerEvent` |
| 4.5 カーネル | `crates/kernel` | 一点集中型の中枢。ブートローダー → カーネル → 各層。下位（`vr` / `overlay`）の状態を 1 か所で持ち、ランタイムの選択（`Kernel::boot`）・フレームの進行（`tick`）・パネル操作（`apply` / `status`）・描画の受け渡し（`submit`）を提供する。UI・画面はカーネルだけを見る。カーネル内の設定 `kernel::config` が持つ設定ファイル: `os::config_dir()/config.toml`（ログと同じ `nyaw` ディレクトリ: Windows `%LOCALAPPDATA% | `Kernel::{boot, with_backend, tick, submit, apply, status, add_panel}`, `PanelCommand`, `PanelStatus`, `config::{load, parse, path, Config}`, `vr` / `overlay` の型の再エクスポート |
| 5 UI | `crates/ui` | GPUI フロントエンド（1つ）。ポインタイベントを GPUI のマウス入力に変換して画面へ流し、描画結果を読み戻してパネルへ返す。どの画面かは知らない | `run(kernel, panels, profiler)`, `PanelSpec`, `pump`（入力1フレーム分。本番と testkit 共通）, `sync_panel`（画面からのパネル操作を反映。本番と testkit 共通）, `capture`（描画の読み戻し）, `open_panel_window`, `drag_preview`（`on_drag` の見た目はこれで包む。VR ではカーソル側だけに描く）, `PointerBridge`, `PanelControl` / `PanelCommand` / `PanelStatus` |
| 5 Web UI | `crates/webui` | UI 層と同格の Tauri フロントエンド（`--webui` で UI 層の代わりに使う）。パネルを WebView のウィンドウにし、ポインタイベントを Tauri のイベントで送って注入したブリッジ（`src/bridge.js`）が DOM の PointerEvent / MouseEvent / click / wheel にする。ページからカーネルへは Tauri のコマンド。`Surface::Overlay` では WebView を撮って（Linux: WebKitGTK のスナップショット、Windows: WebView2 の CapturePreview）パネルへ返す。どのページかは知らない | `run(kernel, panels)` / `run_with`（テスト用の `ready` 付き）, `WebPanelSpec`, `Page`（`nyaw://` で配信）, `WebCommand` / `WebStatus` / `WebDevice` / `WebPointer`, `Outbox`, `BRIDGE` |
| 5 横 | `crates/profiler` | 計測中だけ動くプロファイラ（`--profile` で起動時から、または Testing の Profiler ボタン / `PanelControl::profiling` で切り替え）。UI 層のフレームループが段階ごとの所要時間（wait / input / draw / render / upload と、プロファイラ表示自身の描画 profiler）を記録し、FPS・フレーム時間・CPU/GPU・ドロップをプロファイラパネル（Close ボタンで計測ごと止める）にグラフで出す。`--profile` のときは 1 フレーム 1 行の CSV（`os::log_dir()/profile.csv`）にもダンプする。GPU は同期して待つ段階（描画+読み戻し、VR への転送）の実時間 | `Profiler::{new, with_budget, record, add_self_time, summary}`, `ProfilerView`, `Stages`, `CSV_HEADER` |
| 6 画面 | `crates/testing` ほか（n 個） | UI 層の上に載る画面。GPUI のビューで、ランタイムを意識しない | `Testing`（テスト用の画面） |
| 6 画面 | `crates/wrist` | 常駐パネル。左手首に腕時計のように取り付き（掴めない）、時刻と日付、Toggle Panel ボタンで Testing を召喚 / 閉じる | `wrist::spec()`, `Wrist`, `WATCH_OFFSET` |
| 6 画面 | `crates/beam` | コントローラのビーム。手ごとにレーザー（線）とカーソル（パネルに貼り付く円）とドラッグの見た目（ドラッグ中だけ。中身は `ui::drag_preview` で包んだビューを GPUI が描き、パネルの外・ウィンドウをまたいでも見える）の 3 枚。見た目だけを描き、置き場所・長さ・表示・描画順は `PanelSpec::pointer` で役割を与えたオーバーレイ層が毎 tick 決める（ヒット判定の対象外）。VR（`Surface::Overlay`）のときだけ載せる | `beam::specs()`, `Laser`, `Cursor`, `DragLayer` |
| 6 画面（Web） | `crates/webtesting` | Testing の Web 版（1 ページの HTML）。Web UI 層の機能を試す。`webtesting:probe` イベントに状態と要素の位置を `webtesting:state` で返す（E2E 用） | `webtesting::{page, spec}`, `PROBE_EVENT`, `STATE_EVENT` |
| 6 画面（Web） | `crates/webbeam` | ビームの Web 版（レーザーとカーソルの 2 枚。ドラッグの見た目は無い）。VR のときだけ載せる | `webbeam::specs()` |
| - | `crates/testkit` | Playwright 風 E2E テスト基盤。開発専用。ランタイムごとに対等なキット | `Driver::{click, hover, down, up, drag, scroll, read, color_at}`（全キット共通）, `on_every_runtime!`, `NoVrKit::{with_backend, select_panel, type_keys}`, `VrKit::{panel, add_panel, press, drag_panel, move_hand, hand, profile, point_at}`, `runtimes::{install, launch, Launched::connect}`（実ランタイムの用意・起動・後片付け） |
| - | `src/main.rs` | ブートローダー。ロガーを組み立て、設定を読み、カーネルを起こして（ランタイムの選択はカーネルの仕事）載せる画面（wrist、VR ならビームも）とともに `ui::run` に引き渡すだけ。`--webui` のときは `webui::run` に Web の画面（webtesting、VR なら webbeam も）を引き渡す。`--novr` / `--openxr` / `--openvr`、指定なしで VR が無ければ novr | — |

依存は**必ず下向き**、かつ原則として 1 つ下の層まで（`os ← vr ← input ← overlay ← kernel（config を内に持つ） ← ui ← 画面（testing 等） ← main`。Web は `kernel ← webui ← Web の画面（webtesting 等） ← main` で、`ui` と `webui` は互いに依存しない）。UI・画面・testkit が見る下位の型は `kernel` の再エクスポート経由で、`vr` / `overlay` を直接組み立てない。overlay は `vr` の型（`Pose` 等）を共通語彙として使ってよい。
`logger` は例外で、どの層からも直接依存してよい唯一の横断クレート（`logger` 自身は他の自前クレートに依存しない）。

## ログ

- ログは**必ず** `logger` のマクロで出す。`println!` / `eprintln!` は使わない（テストハーネスの出力は除く）。
  ```rust
  logger::warn!("パネルの描画を取得できない", error = e, panel = ?panel); // key = Display, key = ?Debug
  ```
- 出力は 1 行 1 件の logfmt: `ts=2026-10-13T09:00:00.123Z level=warn target=ui msg=パネルの描画を取得できない error="device lost" panel=0`。
  `ts level target msg` は固定、以降はフィールドを書いた順。キーは snake_case。`target` は `module_path!()`。
- 出力先は `main` だけが組み立てる: 標準エラー出力（`logger::StderrSink`）と `os::log_dir()/nyaw.log`（`os::FileSink`。Windows `%LOCALAPPDATA%\nyaw\logs`、Linux `$XDG_STATE_HOME/nyaw/logs`）。
  起動ごとに新しく作り、前回分は `nyaw-prev.log` に 1 世代だけ残す（既存の prev は消してから退避）。
  ファイル入出力は OS 層の責務なので、新しい出力先が要るときも `Sink` の実装は `os` に置く。
- レベルは環境変数 `NYAW_LOG`（`trace`/`debug`/`info`/`warn`/`error`、既定 `info`）。`init` 前のログは stderr・info 以上に出る。
- 外部クレートの `log` 出力（gpui 等）も同じロガーに流れる。パニックも `main` のフックで `error` として記録する。

## ランタイム（設計思想）

novr・SteamVR (OpenVR)・OpenXR はどれも対等な「手段」でしかない。novr はテスト用の抜け道ではなく、
ランタイム層（`crates/vr`）に OpenVR / OpenXR と並んで常に存在し、ビルドしたバイナリでも常に使える。

- **基本操作はどのランタイムでもネイティブに行える**こと。VR 固有の機能（手・コントローラ機種・パネルを掴んで動かす等）
  が novr で使えないのは構わない。
- ランタイムごとの違いは `VrBackend::surface()` が返す `Surface` だけで表し、上位層はそれ以外で分岐しない。
  - `Surface::Overlay`（OpenVR / OpenXR）: パネルは非表示ウィンドウで描き、読み戻して VR のオーバーレイへ送る。入力はコントローラのレーザー。
  - `Surface::Desktop`（novr）: パネルは OS のウィンドウとして表示する。入力は OS のマウスがネイティブに届く。
- ビューはランタイムを意識しない（どのランタイムでも同じビューが動く）。

## 1 フレームの流れ（`ui::run`。どのランタイムでも同じ。novr ではコントローラが無いので入力は空、送信もしない）

```
ui::run（VR はランタイムの vsync、novr は主ディスプレイのリフレッシュレート。取れなければ約 90Hz）/ testkit の VrKit::step
 └ ui::pump → kernel.tick()（内部で overlay.tick(vr)）
     ├ vr.poll()            → Frame { hmd, controllers: Vec<RawController>, trackers }
     ├ 取り付け中のパネル   → 姿勢 = frame.pose(device) * offset（掴み中は手に従い、離すとオフセットを付け直す）
     ├ input::map(raw)      → ControllerState { aim, select, grab, menu, scroll }
     ├ レーザー × パネル判定 → Vec<UiEvent { panel, hand, PointerEvent }>
     ├ ポインタのパネル     → レーザーをヒット点まで伸ばし、カーソルをヒット点へ（無ければ隠す）
     └ vr.set_overlay_pose  （掴み移動を反映）
 └ 手・パネルごとの PointerBridge::translate → gpui::PlatformInput → 対象パネルの window.dispatch_event
 └ ui::sync_panel           → ビューが PanelControl の PanelId ごとに積んだ操作（取り付け・取り外し・ロック）を kernel に反映し、状態を書き戻す
 └ PanelControl::spawns     → 画面から要求された新しいパネルとウィンドウを開く
 └ ui::capture(window)      → Frame（RGBA、デバイスピクセル）   ※Surface::Overlay のみ。testkit は testkit::raster::capture
                              本番は入力・状態の変化があったとき、または 250ms ごとだけ描く（GPU の完了待ちが重い）。1x1 は送らない
 └ overlay.submit(vr, ...)  → vr.submit_overlay_pixels
```

パネル用ウィンドウは `ui::open_panel_window(cx, surface, ..)` で開く。`Overlay` では非表示で、
`Window::render_to_image` で画面に出さずに描画して読み戻す。`Desktop` では普通に表示する。ルートビューを借用したまま描画・入力すると
二重借用で落ちるので、ウィンドウ操作は `WindowHandle::update` ではなく `cx.update_window(handle.into(), ..)` を使う。

## 画面（UI 層の上。UI 層 1 : 画面 n）

UI 層（`crates/ui`）は1つで、どの画面かを知らない。画面はその上に何個でも載る別クレートで、`ui::run(kernel, Vec<PanelSpec>)` /
testkit のキットに `build` として渡す。画面からパネルへの操作は GPUI のグローバル `PanelControl` に PanelId ごとに `PanelCommand`
（取り付け・取り外し・ロック）を積み、フレームループが `sync_panel` で反映して `PanelStatus`（ランタイム・取り付け先・ロック・
見えているトラッカー・メニューボタン・掴み中）を書き戻す。画面の型に依存しないので、本番・どのキットでも同じ経路。
新しい画面 = `crates/<名前>` を足して `ui` に依存し、`main` で選ぶ。

### Testing（`crates/testing`。テスト用の画面）

一連の機能を1画面で試せる画面。新しい機能を足したら Testing にも操作を足し、E2E はその操作で書く。有効な状態は `BUTTON_ACTIVE_BG` で光る。

| 区分 | 要素（`debug_selector`） | 試せること |
|---|---|---|
| VR | `click-me` | クリック・ホバー（全機種・左右の手） |
| VR | `spawn-panel` | 隣に Testing パネルを追加し、その画面を独立して操作する |
| VR | `pointer-test` | 隣にポインタの方式を試す画面（`PointerTest`）を開く。`pointer-mode` でマウス / タッチのエミュレートを切り替え（タッチで光る）、`drag-chip` → `drop-zone`（マウスのドラッグ）・`swipe-list`（タッチのスワイプ）・`tap-me`（どちらでも押せる） |
| VR | `attach-hmd` / `attach-left` / `attach-right` / `attach-tracker` / `detach` | 取り付け・取り外し（トラッカーは見えている最初のもの） |
| VR | `lock` | 掴めないようにする |
| VR | `menu-held` / `grabbed` | メニューボタン・パネルを掴んでいる間光る（表示のみ） |
| GPUI | `toggle` | 切り替え（`on_click` と状態の見た目） |
| GPUI | `hold-me` | 押している間の見た目（`.active()`） |
| GPUI | `drag-chip` → `drop-zone` | ドラッグ＆ドロップ（`on_drag` / `drag_over` / `on_drop`） |
| GPUI | `open-menu` → `menu-a` / `menu-b` | ポップアップ（`deferred(anchored())`、`on_mouse_down_out` で閉じる） |
| GPUI | `text-field` | フォーカス（`track_focus` / `.focus()`）と文字入力（`on_key_down`。キーボードは novr のみ） |
| GPUI | `scroll-list` | スクロール（`on_scroll_wheel` / `overflow_y_scroll` + `ScrollHandle`） |

ポインタの方式（`kernel::PointerMode`。カーネルが持ち、`config.toml` の `pointer_mode` が初期値、`PanelCommand::PointerMode` で切り替え、`PanelStatus::pointer_mode` で見える。全パネル共通）:
- マウス（既定）: 押したまま動かすとドラッグ。
- タッチ: 押したまま `ui::SWIPE_SLOP`（12px）より動かすとスワイプ（GPUI にはピクセル単位の `ScrollWheel`、宛先は押した所）、動かさずに離すとタップ（その時点で押して離す）。押下はどちらと決まるまで送らないので、`on_drag` のドラッグや押している間の見た目は効かない。方式は押した瞬間に決まり、離すまで変わらない。

未対応: ツールチップ（遅延タイマー）、右クリック・ダブルクリック（レーザーは左ボタン1回押しのみ）、文字・画像の見た目（testkit のラスタライザは矩形・角丸・枠線・下線を描く）。

## Web UI 層（`crates/webui`。UI 層と同格、オーバーレイ層（カーネル）の上）

UI 層（GPUI）の代わりに Tauri の WebView でパネルを出す。`cargo run -- --webui [--novr|--openxr|--openvr]`。
GPUI と Tauri はどちらもメインスレッドのイベントループを握るので、1 プロセスではどちらか一方だけを使う。

```
webui::run（Tauri のイベントループ。別スレッドが周期ごとに run_on_main_thread で起こし、MainEventsCleared でフレームを進める）
 └ kernel.tick()                       … UI 層と同じ（poll → 追従 → ヒット判定 → UiEvent）
 └ Outbox（パネルごと）                 → 同じ位置の Move は捨て、ページの返事（pointer_ack）待ちの間は Move は最新だけ・Scroll は合算
 └ emit_to(panel-N, "nyaw:pointer", [WebPointer])
       └ bridge.js（全パネルに注入）    → DOM の pointerover/enter/down/move/up/out/leave/cancel、mouse*、click、wheel（既定動作のスクロール）、
                                          フォーカス、setPointerCapture（合成の pointerId 1001/1002 を自前で持つ）
 └ ページのコマンド（panel_command / panel_status / spawn_panel）を反映 → 状態が変われば emit_to(panel-N, "nyaw:status", WebStatus)
 └ capture（Surface::Overlay のみ）     → 入力・状態の変化から 300ms は毎フレーム、ほかは 250ms ごとに WebView を撮り、変わっていれば kernel.submit
 └ WindowEvent::Resized                 → PanelCommand::Resize（論理 px。1m あたりのピクセル数を保つので VR 上のパネルも広がる）
```

- ページ（`Page`）は画面のクレートが `include_str!` で持ち、`nyaw://localhost/<名前>/index.html`（Windows は `http://nyaw.localhost/..`）で配信する。
  ページは普通の Web ページと同じく `addEventListener` で書き、カーネルへは `window.nyaw` で話す:
  `nyaw.command("detach" | "close" | { attach: "hmd" | "left" | "right" | { tracker: id } } | { lock: bool } | { resize: { width, height } } | { pointer_mode: "mouse" | "touch" })`,
  `nyaw.status()`, `nyaw.onStatus(cb)`, `nyaw.spawn(ページ名)`。
- イベントの購読は `getCurrentWebviewWindow().listen` で行う（`event.listen` は既定で全宛先を受けるので、ほかのパネル宛ての `emit_to` まで届く）。
- `Surface::Overlay` のウィンドウはデスクトップに出さない。WebKitGTK は未マップのウィンドウを描かないので、Linux は表示したまま画面の外（-20000, -20000）に置く。ほかは非表示。
- novr（`Surface::Desktop`）では WebView を普通のウィンドウとして出し、OS のマウス・キーボードがそのまま届く（ブリッジのポインタは来ない）。
- ポインタの方式がタッチ（`WebPointer::mode` が `"touch"`）のとき、ブリッジはブラウザのタッチと同じ順序で流す: 押すと pointerdown（pointerType `touch`）、12px より動かすと pointercancel して一番近いスクロールできる祖先をスクロール（`touch-action: none` の上では pointermove のまま）、動かさずに離すと pointerup → 互換の mousedown / mouseup → click。触れていない間はホバーのイベントを出さず `data-nyaw-hover` だけ付ける。TouchEvent（touchstart 等）と慣性スクロールは無い。
  Testing (Web) の `open-pointer` で同じパネルに `pointer.html`（Pointer test。`pointer-mode` / `tap-me` / `drag-chip` → `drop-zone` / `swipe-list`）を開く。
- 合成イベントの限界: `isTrusted` は偽、CSS の `:hover` / `:active` は効かない（代わりにブリッジが `data-nyaw-hover` / `data-nyaw-active` を付けるので
  `:is(:hover, [data-nyaw-hover])` と書く）、HTML5 のドラッグ＆ドロップ（`draggable`）とテキスト選択は起きない（ポインタイベントで書いたドラッグは動く）。

## 規約

- 語彙は OpenXR に合わせる: プロファイル `"valve/index_controller"`、入力 `"trigger/value"`。OpenVR バックエンドが翻訳する。
- 座標: トラッキング空間はメートル・前方 -Z。パネル表面はローカル +Z。パネルのピクセル座標は左上原点。
- 新しいコントローラ対応 = `crates/input` の `PROFILES` に 1 行追加。上位層でプロファイル名を見ない。
- 新しいランタイム = `crates/vr` に `VrBackend` 実装を追加し `connect` に登録。`surface()` で見せ方を宣言する。
- `CODE_OF_CONDUCT.md` に従う: コメントは日本語、Conventional Commits、CalVer（現在 `2026.0.1`、`[workspace.package]` で一括管理）、単体テスト必須、層をまたぐものは結合テスト。
- `// ponytail:` コメントは意図的な簡略化。限界と次の一手が書いてある。

## テスト

`cargo test --workspace`。ビルドには cmake と C++ コンパイラが要る（`vr` が OpenXR ローダーを静的リンクし、`openvr_bindings` もビルド時に OpenVR を cmake でビルドするため）。cmake が PATH に無ければ環境変数 `CMAKE` で指す（Windows は Visual Studio 同梱の `Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe` でよい）。CI は `cargo fmt --check` と `cargo clippy --workspace --all-targets -D warnings`。

- **単体テスト**: 各クレート内の `#[cfg(test)]`。その層の範囲だけで完結させる。実機の代わりは `vr` の `test-support` フィーチャーで出る `vr::mock::MockBackend`。単体テストから testkit は使わない（依存が循環する）。
- **ユーザー操作のシナリオ（E2E 風）**: `crates/testkit/tests/` に置く。Testing の画面を操作して検証する（取り付け・ロックも `kit.click("attach-hmd")` のようにボタンで）。testkit 自体は画面を知らず、`testing` は dev-dependency。要素は GPUI の `.debug_selector(|| "名前".into())` で指定し、プロファイル名や座標計算はテストに書かない。
  - **基本操作**（クリック・ホバー・見えている色など）: `Driver` に対して1回だけ書き、`on_every_runtime!` で全ランタイム（novr / VR）で実行する（例: `testing.rs`）。
  - **UI のロジックだけ**の確認: ランタイムに関係しないので `NoVrKit` だけで書く。
  - **VR 固有の機能**（レーザー・機種・手・パネル移動・VR に映る描画）: `VrKit` で書く（例: `vr_controls.rs`, `panel_texture.rs`, `attach.rs`, ポインタの方式は `pointer_mode.rs`）。
  - `color_at` は「ユーザーに見えている色」: novr は画面、VR は VR パネルに送られたテクスチャ。テストプラットフォームには GPU が無いため、描画は `testkit::raster` の CPU ラスタライザ。
- **実 GPU での読み戻し**: `crates/ui/tests/gpu_capture.rs`（`harness = false`、本番と同じプラットフォームで非表示ウィンドウを描いて読み戻す。Windows のみ）。
- **WebView の E2E**: `crates/webui/tests/webview_vr.rs` / `webview_novr.rs`（`harness = false`。Tauri をメインスレッドで回し、手順は別スレッドで流す。ページの状態は webtesting の `webtesting:probe` で読む）。
  - `webview_vr`: モックの VR（`Surface::Overlay`）。仮想コントローラのレーザーでホバー・クリック・ドラッグ（setPointerCapture）・スクロール・フォーカス・カーネル操作（ロック・取り付け）・パネルを掴む・リサイズ・隣にパネルを開いて閉じるまでを JS 側のイベントで確かめ、読み戻した描画が VR パネルに届く（クリックで変わった色・リサイズ後の大きさ）ことも見る。
  - `webview_pointer`: モックの VR で Pointer test を開き、既定のマウスでは押したまま動かしてもスクロールしないこと、タッチに切り替えるとスワイプでスクロール（pointercancel）・タップで click・`touch-action: none` の上ではドラッグが続くことを確かめる（Windows の WebView2 でも確認済み）。
  - `webview_novr`: novr。xdotool で OS のマウスを動かし（`isTrusted` なネイティブのイベント）、OS 側のウィンドウのリサイズにカーネルの解像度が追従することも見る。
  - Linux は画面（X11。Xvfb でよい）があれば `cargo test --workspace` で流れ、無ければスキップ。ほかの OS は `-- --ignored` のときだけ。ビルドには WebKitGTK（`libwebkit2gtk-4.1-dev`）が要る。
    ```sh
    Xvfb :99 & DISPLAY=:99 cargo test -p webui --tests
    ```
- **実ランタイムでの E2E**: `crates/testkit/tests/runtimes.rs`（`harness = false`）。`testkit::runtimes` がランタイムを起動し（Playwright の `webServer` に当たる）、繋いでオーバーレイへ送り、Testing をマウスで操作して（取り付け・ロック）その描画を `NoVrKit::with_backend` で実ランタイムへ送り、OpenVR はランタイムから読み戻して（光ったボタンまで）確かめ、Drop で自分が起動したプロセスを止めて消えたことまで確かめる。起動に数秒かかり画面にも出るので `cargo test -p testkit --test runtimes -- --ignored [monado|steamvr]` のときだけ流す。入っていないランタイムと、すでに動いているランタイム（別の設定で動いているかもしれず、止めると持ち主が困る）はスキップする。
  - **ランタイムを入れる**: `cargo runtimes`（`.cargo/config.toml` の別名。`npx playwright install` に当たるウィザード。`list` / `install [名前] [--dry-run]` / `uninstall [名前]`）。入れる先は `NYAW_RUNTIMES_PATH`、無ければ `%LOCALAPPDATA%\nyaw\runtimes`（Linux は `~/.cache/nyaw/runtimes`）。
  - **Monado**: Windows 版は公式の配布物が無いので、Monado の GitLab CI の `windows` ジョブの成果物（main の最新。`install/` に `monado-service.exe` と `openxr_monado.json`）を入れる。`XRT_COMPOSITOR_COMPUTE=false` で起動し（コンピュート合成でクアッドを出すと落ちる）、`XR_RUNTIME_JSON` でマニフェストを指す。模擬 HMD（simulated）の画面にパネルが映る。読み戻しの手段は無い。Linux はディストリのパッケージを使う。ログは `<入れる先>/logs/monado-service.log`。
  - **SteamVR**: Steam の管理下なので入れない（`steam://install/250820` を開くだけ）。HMD が無くても動くよう、null ドライバを有効にした `steamvr.vrsettings` だけを置いた一時フォルダを `VR_CONFIG_PATH` で指す（普段の設定には触らない）。最初に繋いだクライアントが SteamVR を起動する。null ドライバだと `steamvr_room_setup` が立ち上がって vrserver を起動し直すので、後片付けは静かになるまで止め続ける。

```rust
// 基本操作: 全ランタイムで実行（click_button::novr / click_button::vr）
fn click_button(kit: &mut impl Driver<Testing>) {
    kit.click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
on_every_runtime!(click_button, |_, _| Testing::default());

// VR 固有
#[gpui::test]
fn left_touch_controller_clicks(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Testing::default());
    kit.hand(Hand::Left).profile("oculus/touch_controller").click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
```

## 未実装（土台のみ）

- 実ランタイムでの確認の残り。OpenVR（`openvr_bindings` の C API の関数表。openvr_api は jsoncpp が OpenXR ローダーと衝突するのでリンクせず、`openvr/client.rs` が vrclient を直接読む）は null ドライバで接続・オーバーレイへの送信（Vulkan テクスチャ、毎フレーム更新）まで確認済みで、コントローラ入力は未確認。OpenXR（XR_EXTX_overlay + XR_KHR_vulkan_enable2）は Monado（Windows、simulated）でパネルが映るところまで確認済みで、コントローラ入力は未確認。
- Linux での描画の読み戻し（gpui の `render_to_image` が Windows のみ実装。`gpui_wgpu` のヘッドレスレンダラ経由が候補）。
- Web UI 層: Windows の WebView2 の読み戻し（CapturePreview）は型検査のみで実機未確認（非表示ウィンドウで描かれない場合は画面外に置く）。macOS の読み戻しは無い。
  Wayland ではウィンドウを画面外に置けないので VR のパネルがデスクトップにも出る。プロファイラ・ドラッグの見た目（`Pointer::Drag`）は無い。
  キーボードは novr のみ（UI 層と同じ）。
- 描画は毎フレーム CPU に読み戻し、変化があれば `vr` が Vulkan のイメージへ上げ直してランタイムへ渡す（`crates/vr/src/gpu.rs`。1024x768 で約 1.6ms）。GPUI のテクスチャを読み戻さずに直接共有する経路は未実装。
- Windows での OpenXR ランタイム検出（レジストリ）。
- `gpui` は crates.io 版では `Window::dispatch_event` を外部から呼べないため、Zed の git リビジョンに固定している。`render_to_image` のため `ui` は本番でも gpui の `test-support` フィーチャーを有効にしている。
