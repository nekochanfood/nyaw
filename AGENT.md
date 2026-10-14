# AGENT.md

nyaw は GPUI で GUI を描く VR オーバーレイシステム。Cargo ワークスペースで、層ごとに 1 クレート。

## 層構成（下 → 上）

| 層 | クレート | 責務 | 主な公開 API |
|---|---|---|---|
| - | `crates/logger` | logfmt 形式の構造化ロガー（横断）。整形・レベル・出力先 `Sink` の抽象と、`log` クレート（gpui 等）からの橋渡し | `trace!/debug!/info!/warn!/error!`, `Logger`, `Sink`, `StderrSink`, `init`, `format` |
| 0 OS | `crates/os` | Windows/Linux の差を吸収（パス、環境変数、ランタイム検出、ファイル入出力）。`#[cfg]` はここだけに書く | `config_dir`, `log_dir`, `log_level`, `FileSink`, `openvr_paths_file`, `openxr_runtime_manifest` |
yaw`、Linux `$XDG_STATE_HOME/nyaw`）を読む。無ければ既定値、知らないキーは無視。キーは `log`（ログレベル）、`runtime`（`openvr`/`openxr`/`novr`）、`pointer_smoothing`（レーザーの手ブレ抑え）、`pointer_mode`（`mouse`/`touch`。レーザーのエミュレート方式、既定 `mouse`）。優先順位はコマンドライン > 設定ファイル（ログレベルは `NYAW_LOG` > 設定ファイル）で、決めるのは `main` | `load`, `parse`, `path`, `Config` |
| 2 ランタイム | `crates/vr` | OpenVR / OpenXR / novr の差を吸収。上位層は `Box<dyn VrBackend>` しか持たない。novr はデスクトップのウィンドウで VR を模すエミュレーター | `connect`, `VrBackend`, `Runtime`, `novr::{Emulator, Button}`, `Pose`, `Frame`（`pose(&Device)`）, `Device`, `RawController`, `mock::MockBackend`（`test-support` フィーチャー時のみ） |
| 3 入力 | `crates/input` | コントローラ機種の差を吸収（ドライバ的役割）。プロファイル表で生入力 → 抽象アクション | `map(&RawController) -> ControllerState`, `source(profile, Action)` / `scroll_source(profile)`（逆引き） |
| 4 オーバーレイ | `crates/overlay` | パネル配置、デバイスへの取り付け（追従）・掴めないようにするロック（空間固定）、レーザーのヒット判定、ポインタイベント生成、パネルの掴み移動 | `Overlay::{add_panel, attach, detach, set_locked, set_resolution, tick, submit}`, `Panel::pixel_to_world`, `UiEvent`, `PointerEvent` |
| 4.5 カーネル | `crates/kernel` | 一点集中型の中枢。ブートローダー → カーネル → 各層。下位（`vr` / `overlay`）の状態を 1 か所で持ち、ランタイムの選択（`Kernel::boot`）・フレームの進行（`tick`）・パネル操作（`apply` / `status`）・描画の受け渡し（`submit`）を提供する。UI・画面はカーネルだけを見る。カーネル内の設定 `kernel::config` が持つ設定ファイル: `os::config_dir()/config.toml`（ログと同じ `nyaw` ディレクトリ: Windows `%LOCALAPPDATA% | `Kernel::{boot, with_backend, tick, submit, apply, status, add_panel}`, `PanelCommand`, `PanelStatus`, `config::{load, parse, path, Config}`, `vr` / `overlay` の型の再エクスポート |
| 5 UI | `crates/ui` | UI 層（1つ）。パネルを GPUI のウィンドウか Tauri の WebView（`ui::web`）で実体化し、1 つのフレームループ・1 つのカーネルで両方を回す。GPUI はポインタイベントをマウス入力に変換して画面へ流し、Web は注入したブリッジ（`src/web/bridge.js`）が DOM のポインタイベントにする。描画結果を読み戻してパネルへ返す。どの画面・ページかは知らない | `run(kernel, panels, profiler, web_files)` / `run_with`（テスト用の `Hooks`）, `PanelSpec`（`new` = GPUI、`web` / `spawn_web` = Web のページ）, `pump` / `pump_with`（入力1フレーム分。本番と testkit 共通）, `sync_panel`（画面からのパネル操作を反映。本番と testkit 共通）, `capture`（描画の読み戻し）, `open_panel_window`, `drag_preview`（`on_drag` の見た目はこれで包む。VR ではカーソル側だけに描く）, `PointerBridge`, `PanelControl` / `PanelCommand` / `PanelStatus`, `web::{Host, Files, WebCommand, WebStatus, WebPointer, Outbox, BRIDGE}` |
| 5 横 | `crates/profiler` | 計測中だけ動くプロファイラ（`--profile` で起動時から、または Testing の Profiler ボタン / `PanelControl::profiling` で切り替え）。UI 層のフレームループが段階ごとの所要時間（wait / input / draw / render / upload と、プロファイラ表示自身の描画 profiler）を記録し、FPS・フレーム時間・CPU/GPU・ドロップをプロファイラパネル（Close ボタンで計測ごと止める）にグラフで出す。`--profile` のときは 1 フレーム 1 行の CSV（`os::log_dir()/profile.csv`）にもダンプする。GPU は同期して待つ段階（描画+読み戻し、VR への転送）の実時間 | `Profiler::{new, with_budget, record, add_self_time, summary}`, `ProfilerView`, `Stages`, `CSV_HEADER` |
| 6 画面 | `crates/testing` ほか（n 個） | UI 層の上に載る画面。GPUI のビューで、ランタイムを意識しない | `Testing`（テスト用の画面） |
| 6 画面 | `crates/wrist` | 常駐パネル。左手首に腕時計のように取り付き（掴めない）、時刻と日付、Toggle Panel ボタンで Testing を召喚 / 閉じる | `wrist::spec()`, `Wrist`, `WATCH_OFFSET` |
| 6 画面 | `crates/beam` | コントローラのビーム。手ごとにレーザー（線）とカーソル（パネルに貼り付く円）とドラッグの見た目（ドラッグ中だけ。中身は `ui::drag_preview` で包んだビューを GPUI が描き、パネルの外・ウィンドウをまたいでも見える）の 3 枚。見た目だけを描き、置き場所・長さ・表示・描画順は `PanelSpec::pointer` で役割を与えたオーバーレイ層が毎 tick 決める（ヒット判定の対象外）。どのランタイムでも載せる（novr のレーザーは視線と重なって見えない） | `beam::specs()`, `Laser`, `Cursor`, `DragLayer` |
| 6 画面（Web） | `web/` + `crates/webpages` | Web のページ（React + Tailwind + Base UI。Vite のマルチページ）。`testing`（Testing の Web 版: 基本的なコンポーネントと VR の操作）と `testing-transparent`（半透明の確認）。`webpages` の build.rs が `npm run build` して全ファイルを埋め込む（Node.js が要る。`NYAW_WEB_SKIP=1` で飛ばす）。lint は oxlint、整形は oxfmt | `webpages::{FILES, TESTING, TESTING_TRANSPARENT}`, `web/src/nyaw.ts`（`window.nyaw` の型と `useStatus`）, `web/src/probe.ts`（E2E 用 `testing:probe`） |
| - | `crates/testkit` | Playwright 風 E2E テスト基盤。開発専用。ランタイムごとに対等なキット | `Driver::{click, hover, down, up, drag, scroll, read, color_at}`（全キット共通）, `on_every_runtime!`, `NoVrKit::{screenshot, on_screen, color_on_screen, move_mouse, press, release, stick, select_panel, type_keys}`, `DesktopKit::with_backend`, `VrKit::{panel, add_panel, press, drag_panel, move_hand, hand, profile, point_at}`, `runtimes::{install, launch, Launched::connect}`（実ランタイムの用意・起動・後片付け） |
| - | `src/main.rs` | ブートローダー。ロガーを組み立て、設定を読み、カーネルを起こして（ランタイムの選択はカーネルの仕事）載せる画面（wrist とビーム）と Web のページ（`webpages::FILES`）とともに `ui::run` に引き渡すだけ。`--novr` / `--openxr` / `--openvr`、指定なしで VR が無ければ novr | — |

依存は**必ず下向き**、かつ原則として 1 つ下の層まで（`os ← vr ← input ← overlay ← kernel（config を内に持つ） ← ui ← 画面（testing 等） ← main`）。Web のページ（`webpages`）は何にも依存せず、`main` が `ui::run` に渡す。画面は Web のページを名前（`PanelSpec::spawn_web(.., "testing")`）で開く。UI・画面・testkit が見る下位の型は `kernel` の再エクスポート経由で、`vr` / `overlay` を直接組み立てない。overlay は `vr` の型（`Pose` 等）を共通語彙として使ってよい。
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
- **上位層はランタイムで分岐しない**。どのランタイムでもパネルは非表示ウィンドウ（GPUI / WebView。実質ヘッドレス）で描き、
  読み戻してオーバーレイへ送る。入力はコントローラのレーザー。
- **novr は VR のエミュレーター**（`vr::novr::Emulator`）。最初の poll でウィンドウ（`nyaw (novr)`）を開き、受け取ったテクスチャを
  固定した頭（原点・前方 -Z、縦の視野 70°）から見た絵として合成する。重なりは VR と同じく描画順。背景は透過がわかるよう無彩色の市松模様。
  ウィンドウを閉じると終了を求める（`Frame::quit`）。poll は約 90Hz に待って周期を作る（VR の vsync 待ちに当たる）。
  - 片手操作: 右手のレイは目からマウスの方向（レーザーは視線と重なるので見えず、カーソルだけ見える）。左クリック = トリガー、
    右クリック = グリップ（掴む）、矢印キー = 右手のスティック。左手は手元で下を向いていてパネルを指さず、WASD = 左手のスティック。
  - デバイスは実在しないので取り付けたパネルは追従しない（`Frame::emulated` で `Frame::pose` が `None`。取り付け先を見失ったのと同じ扱い）。
  - キーボードはスティックなのでパネルには届かない（VR と同じ）。
  - Linux の GPUI は `render_to_image` が未実装なので、novr でも GPUI のパネルは映らない（WebView は映る）。
- ビューはランタイムを意識しない（どのランタイムでも同じビューが動く）。

## 1 フレームの流れ（`ui::run`。どのランタイムでも同じ）

```
ui::run（周期は poll が作る: VR はランタイムの vsync、novr は約 90Hz）/ testkit の VrKit::step・NoVrKit::step
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
 └ ui::capture(window)      → Frame（RGBA、デバイスピクセル）   testkit は testkit::raster::capture
                              本番は入力・状態の変化があったとき、または 250ms ごとだけ描く（GPU の完了待ちが重い）。1x1 は送らない
 └ overlay.submit(vr, ...)  → vr.submit_overlay_pixels
```

パネル用ウィンドウは `ui::open_panel_window(cx, surface, ..)` で開く。`Overlay` では非表示で、
`Window::render_to_image` で画面に出さずに描画して読み戻す。ルートビューを借用したまま描画・入力すると
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
| Web | `open-web` | 隣に Testing の Web 版（`web/testing/`。React + Base UI）を開く。Web 版には同じ操作と、Base UI の Switch / Checkbox / Slider / Tabs、半透明のページ（`testing-transparent`）を開くボタンがある |
| VR | `spawn-panel` | 隣に Testing パネルを追加し、その画面を独立して操作する |
| VR | `pointer-test` | 隣にポインタの方式を試す画面（`PointerTest`）を開く。`pointer-mode` でマウス / タッチのエミュレートを切り替え（タッチで光る）、`drag-chip` → `drop-zone`（マウスのドラッグ）・`swipe-list`（タッチのスワイプ）・`tap-me`（どちらでも押せる） |
| VR | `attach-hmd` / `attach-left` / `attach-right` / `attach-tracker` / `detach` | 取り付け・取り外し（トラッカーは見えている最初のもの） |
| VR | `lock` | 掴めないようにする |
| VR | `menu-held` / `grabbed` | メニューボタン・パネルを掴んでいる間光る（表示のみ） |
| GPUI | `toggle` | 切り替え（`on_click` と状態の見た目） |
| GPUI | `hold-me` | 押している間の見た目（`.active()`） |
| GPUI | `drag-chip` → `drop-zone` | ドラッグ＆ドロップ（`on_drag` / `drag_over` / `on_drop`） |
| GPUI | `open-menu` → `menu-a` / `menu-b` | ポップアップ（`deferred(anchored())`、`on_mouse_down_out` で閉じる） |
| GPUI | `text-field` | フォーカス（`track_focus` / `.focus()`）と文字入力（`on_key_down`。キーボードは無いので testkit の `NoVrKit::type_keys` で GPUI に直接打って確かめる） |
| GPUI | `scroll-list` | スクロール（`on_scroll_wheel` / `overflow_y_scroll` + `ScrollHandle`） |

ポインタの方式（`kernel::PointerMode`。カーネルが持ち、`config.toml` の `pointer_mode` が初期値、`PanelCommand::PointerMode` で切り替え、`PanelStatus::pointer_mode` で見える。全パネル共通）:
- マウス（既定）: 押したまま動かすとドラッグ。
- タッチ: 押したまま `ui::SWIPE_SLOP`（12px）より動かすとスワイプ（GPUI にはピクセル単位の `ScrollWheel`、宛先は押した所）、動かさずに離すとタップ（その時点で押して離す）。押下はどちらと決まるまで送らないので、`on_drag` のドラッグや押している間の見た目は効かない。方式は押した瞬間に決まり、離すまで変わらない。

未対応: ツールチップ（遅延タイマー）、右クリック・ダブルクリック（レーザーは左ボタン1回押しのみ）、文字・画像の見た目（testkit のラスタライザは矩形・角丸・枠線・下線を描く）。

## Web のパネル（`ui::web`。UI 層の中で GPUI と並ぶ）

UI 層は 1 つで、パネルの中身を GPUI のビュー（`PanelSpec::new`）か Web のページ（`PanelSpec::web` / `spawn_web`）から選ぶ。
GPUI と Tauri はどちらもメインスレッドのイベントループを使うので、GPUI のループ（`ui::run`）の上で Tauri を毎フレーム 1 回ずつ回す
（`App::run_iteration`。Linux は GTK の溜まったイベントも処理する）。Tauri は最初の Web のパネルを開くときに起こす。
カーネルは 1 つで、GPUI のパネルと Web のパネルは同じ PanelId の空間に並ぶ。Testing（GPUI）の `open-web` ボタンで Testing の Web 版を開ける。

```
ui::run の 1 フレーム（GPUI の部分は上の「1 フレームの流れ」と同じ）
 └ pump_with                            → GPUI のパネル宛ては PlatformInput に、Web のパネル宛ては UiEvent のまま web::Host へ
 └ web::Host::deliver                   → Outbox（パネルごと）: 同じ位置の Move は捨て、ページの返事（pointer_ack）待ちの間は Move は最新だけ・Scroll は合算
 └ web::Host::step
     ├ Tauri を 1 回回す（run_iteration）、ウィンドウの Resized / Destroyed を処理
     ├ emit_to(panel-N, "nyaw:pointer", [WebPointer])
     │    └ bridge.js（全ページに注入）  → DOM の pointerover/enter/down/move/up/out/leave/cancel、mouse*、click、wheel（既定動作のスクロール）、
     │                                      フォーカス、setPointerCapture（合成の pointerId 1001/1002 を自前で持つ）
     ├ ページのコマンド（panel_command / panel_status / spawn_panel）を反映 → 状態が変われば emit_to(panel-N, "nyaw:status", WebStatus)
     ├ capture                          → 入力・状態の変化から 300ms は毎フレーム、ほかは 250ms ごとに WebView を撮り、変わっていれば kernel.submit
     └ ページが開くよう頼んだページ       → PanelSpec::spawn_web で開く
```

- ページは `web/`（Vite + React + TypeScript + Tailwind v4 + Base UI）。`web/<ページ名>/index.html` が入口で、`vite.config.ts` の `pages` に足す。
  画面ごとに `web/src/<ページ名>/App.tsx` を持ち、共通の `main.tsx` と `router.tsx`（TanStack Router のコードベースルーティング）で起動する。
  `crates/webpages` の build.rs がビルドして埋め込み、UI 層は `nyaw://localhost/<ページ名>/index.html`（Windows は `http://nyaw.localhost/..`）で配信する。
  `cd web && npm run check` で oxlint（`--deny-warnings`）・oxfmt・tsc・ビルドをまとめて流す（CI の `web` ジョブと同じ）。lint と整形は `crates/ui/src/web/bridge.js` も対象。
- ページからカーネルへは `window.nyaw`（`web/src/nyaw.ts` に型と `useStatus`）:
  `nyaw.command("detach" | "close" | { attach: "hmd" | "left" | "right" | { tracker: id } } | { lock: bool } | { resize: { width, height } } | { pointer_mode: "mouse" | "touch" })`,
  `nyaw.status()`, `nyaw.onStatus(cb)`（戻り値で購読をやめる）, `nyaw.spawn(ページ名)`。
- リサイズ: `{ resize }` でウィンドウの大きさが変わると `PanelCommand::Resize` で解像度を合わせる（1m あたりのピクセル数を保つので VR 上のパネルも広がる）。
- イベントの購読は `getCurrentWebviewWindow().listen` で行う（`event.listen` は既定で全宛先を受けるので、ほかのパネル宛ての `emit_to` まで届く）。
- ウィンドウは（novr でも）デスクトップに出さず、GPUI のパネルと同じく背景を透かす（塗るかどうかはページが決める。読み戻しはストレートアルファ）。
  WebKitGTK は未マップのウィンドウを描かないので、Linux は表示したまま画面の外（-20000, -20000）に置く。ほかは非表示。
- 合成イベントの限界（VR）: `isTrusted` は偽、CSS の `:hover` / `:active` は効かない。代わりにブリッジが `data-nyaw-hover` / `data-nyaw-active` を付け、
  `web/src/index.css` で Tailwind の `hover:` / `active:` をこの属性でも効くよう上書きしている。HTML5 のドラッグ＆ドロップ（`draggable`）とテキスト選択は起きない
  （ポインタイベントで書いたドラッグや Base UI の Slider は動く）。
- Base UI は Switch / Checkbox / Slider のつまみの `id` を中の隠れた `<input>` に付けるので、E2E で位置を引く `id` は見える要素（包む要素など）に付ける。
- Testing の `open-pointer` で同じパネルの Pointer test（`testing-pointer`）へ移る。マウスはドラッグ、タッチは 12px 以上動かすと `pointercancel` してスワイプになる。
  `touch-action: none` のチップは両方式でドラッグできる。スティックは両方式で縦横にスクロールし、右で右、上で上へ進む。
  `swipe-list` が一次元、`swipe-grid` が二次元のスクロール領域。`crates/ui/tests/web_pointer.rs` がカーネルからの入力経路を検証する。
  `cd web && npm test` は実物のブリッジと React を使うブラウザ回帰テスト（事前に `npm run build` と `npx playwright install chromium`）。

## 規約

- 語彙は OpenXR に合わせる: プロファイル `"valve/index_controller"`、入力 `"trigger/value"`。OpenVR バックエンドが翻訳する。
- 座標: トラッキング空間はメートル・前方 -Z。パネル表面はローカル +Z。パネルのピクセル座標は左上原点。
- 新しいコントローラ対応 = `crates/input` の `PROFILES` に 1 行追加。上位層でプロファイル名を見ない。
- 新しいランタイム = `crates/vr` に `VrBackend` 実装を追加し `connect` に登録。`surface()` で見せ方を宣言する。
- `CODE_OF_CONDUCT.md` に従う: コメントは日本語、Conventional Commits、CalVer（現在 `2026.0.1`、`[workspace.package]` で一括管理）、単体テスト必須、層をまたぐものは結合テスト。
- `// ponytail:` コメントは意図的な簡略化。限界と次の一手が書いてある。

## テスト

E2E はローカルで実行し、CI には追加しない。

`cargo test --workspace`。ビルドには cmake と C++ コンパイラが要る（`vr` が OpenXR ローダーを静的リンクし、`openvr_bindings` もビルド時に OpenVR を cmake でビルドするため）。cmake が PATH に無ければ環境変数 `CMAKE` で指す（Windows は Visual Studio 同梱の `Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe` でよい）。CI は `cargo fmt --check` と `cargo clippy --workspace --all-targets -D warnings`。

- **単体テスト**: 各クレート内の `#[cfg(test)]`。その層の範囲だけで完結させる。実機の代わりは `vr` の `test-support` フィーチャーで出る `vr::mock::MockBackend`。単体テストから testkit は使わない（依存が循環する）。
- **ユーザー操作のシナリオ（E2E 風）**: `crates/testkit/tests/` に置く。Testing の画面を操作して検証する（取り付け・ロックも `kit.click("attach-hmd")` のようにボタンで）。testkit 自体は画面を知らず、`testing` は dev-dependency。要素は GPUI の `.debug_selector(|| "名前".into())` で指定し、プロファイル名や座標計算はテストに書かない。
  - **基本操作**（クリック・ホバー・見えている色など）: `Driver` に対して1回だけ書き、`on_every_runtime!` で全ランタイム（novr / VR）で実行する（例: `testing.rs`）。
  - **UI のロジックだけ**の確認: ランタイムに関係しないので `NoVrKit` だけで書く。
  - `NoVrKit` は本番の novr と同じくエミュレーター（ウィンドウを出さない `Emulator::headless`）を通す。マウス・ボタン・スティックを
    注入して（`move_mouse` / `press(Button::Grab)` / `stick(Hand::Left, ..)`）、ユーザーに見える絵は `screenshot()`（エミュレーターの画面）で見る。
    要素が画面のどこに映るかは `on_screen(selector)`。`NYAW_E2E_WATCH` でエミュレーターの画面をウィンドウに出す（例: `novr_emulator.rs`）。
  - **VR 固有の機能**（レーザー・機種・手・パネル移動・VR に映る描画）: `VrKit` で書く（例: `vr_controls.rs`, `panel_texture.rs`, `attach.rs`, ポインタの方式は `pointer_mode.rs`）。
  - `color_at` は「ユーザーに見えている色」: novr はエミュレーターの画面、VR は VR パネルに送られたテクスチャ。テストプラットフォームには GPU が無いため、描画は `testkit::raster` の CPU ラスタライザ。
- **実 GPU での読み戻し**: `crates/ui/tests/gpu_capture.rs`（`harness = false`、本番と同じプラットフォームで非表示ウィンドウを描いて読み戻す。Windows のみ）。
- **Web のパネルの E2E**: `crates/ui/tests/web_vr.rs` / `web_novr.rs` / `web_pointer.rs`（`harness = false`）。本番と同じ `ui::run_with`（GPUI のループの上で Tauri も回す）を実プラットフォームで起動し、
  手順は別スレッドで流す。ページの状態は `web/src/probe.ts` の `testing:probe` → `testing:state` で読み、GPUI 側は `Hooks::remote` でフレームループに処理を差し込む。
  - `web_vr`: モックの VR。仮想コントローラのレーザーで、ホバー（Tailwind の hover: の色まで VR に映る）・クリック・押している間・
    Base UI の Toggle / Switch / Checkbox / Tabs / Slider（ポインタキャプチャ）/ Menu（ポップアップ）・ドラッグ・スクロール（ScrollArea）・フォーカス・
    カーネル操作（ロック・取り付け）・パネルを掴む・リサイズを JS 側で確かめ、読み戻した描画が VR パネルに届くことを見る。
    半透明のページを開き、テクスチャのアルファ（透明 0・25% 64・50% 128・不透明 255、色はストレートアルファ）とレーザーでの操作、Close で閉じるところまで見る。
  - `web_novr`: novr（エミュレーター、headless）。GPUI の Testing を開き、そこから（ボタンと同じ `PanelControl::spawns` で）Web 版を開き、
    テストのスレッドからエミュレーターへ操作をチャンネルで送って（エミュレーターはメインスレッドから出せない）、マウス = 右手のレイでのホバー・
    左クリック = トリガー・矢印キー = スクロール・右クリック = グリップでパネルを運ぶを確かめ、ページの描画がエミュレーターの画面（市松模様の上）に映ることも見る。
    ボタン自体（GPUI の Testing → Web のページを頼む）は testkit の `open_web_testing` で全ランタイムで確かめる。
  - Linux は画面（X11。Xvfb でよい）があれば `cargo test --workspace` で流れ、無ければスキップ。ほかの OS は `-- --ignored` のときだけ。
    GPUI のウィンドウに Vulkan（GPU が無ければ mesa の lavapipe）、Web に WebKitGTK（`libwebkit2gtk-4.1-dev`）が要る。
    ```sh
    Xvfb :99 & DISPLAY=:99 cargo test -p ui --tests
    ```
- **実ランタイムでの E2E**: `crates/testkit/tests/runtimes.rs`（`harness = false`）。`testkit::runtimes` がランタイムを起動し（Playwright の `webServer` に当たる）、繋いでオーバーレイへ送り、Testing をマウスで操作して（取り付け・ロック）その描画を `DesktopKit::with_backend`（パネルのウィンドウへマウスを直接送るキット）で実ランタイムへ送り、OpenVR はランタイムから読み戻して（光ったボタンまで）確かめ、Drop で自分が起動したプロセスを止めて消えたことまで確かめる。起動に数秒かかり画面にも出るので `cargo test -p testkit --test runtimes -- --ignored [monado|steamvr]` のときだけ流す。入っていないランタイムと、すでに動いているランタイム（別の設定で動いているかもしれず、止めると持ち主が困る）はスキップする。
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
- Web のパネル: Windows はモックの VR で入力・リサイズ・半透明・WebView2 の読み戻し（CapturePreview）を確認済み。
  Tauri は最初の Web のパネルを開くまで起こさないので、開かなければ GPUI だけの動きは変わらない。macOS の読み戻しは無い。
  Wayland ではウィンドウを画面外に置けないので VR の Web のパネルがデスクトップにも出る。Web のページのドラッグの見た目は `Pointer::Drag` のパネルに出ない（ページ内で動かす）。
- 描画は毎フレーム CPU に読み戻し、変化があれば `vr` が Vulkan のイメージへ上げ直してランタイムへ渡す（`crates/vr/src/gpu.rs`。1024x768 で約 1.6ms）。GPUI のテクスチャを読み戻さずに直接共有する経路は未実装。
- Windows での OpenXR ランタイム検出（レジストリ）。
- `gpui` は crates.io 版では `Window::dispatch_event` を外部から呼べないため、Zed の git リビジョンに固定している。`render_to_image` のため `ui` は本番でも gpui の `test-support` フィーチャーを有効にしている。
