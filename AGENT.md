# AGENT.md

nyaw は GPUI で GUI を描く VR オーバーレイシステム。Cargo ワークスペースで、層ごとに 1 クレート。

## 目次（下 → 上）

各クレートの詳細はリンク先の `AGENT.md`。

| 層 | クレート | 一言 |
|---|---|---|
| - | [`crates/logger`](crates/logger/AGENT.md) | logfmt 形式の構造化ロガー（横断） |
| - | [`crates/animation`](crates/animation/AGENT.md) | 動きの責務（Motion / Animation / Transition / Slide）。GPUI にも design にも依存しない |
| Subsystems | [`crates/subsystems`](crates/subsystems/AGENT.md) | OS・ランタイム・入力・描画の差を吸収し、上位に共通フレームを渡す。下の 8 クレートを束ねる |
| Subsystems | [`crates/subsystems/os`](crates/subsystems/os/AGENT.md) | 保存先・ランタイム検出・タイマー精度などの OS 差 |
| Subsystems | [`crates/subsystems/vr`](crates/subsystems/vr/AGENT.md) | OpenVR / OpenXR / novr の差とエミュレーター |
| Subsystems | [`crates/subsystems/input`](crates/subsystems/input/AGENT.md) | コントローラ機種の差（プロファイル表）と骨格の手の操作 |
| Subsystems | [`crates/subsystems/rendering`](crates/subsystems/rendering/AGENT.md) | GPUI の非表示描画先・読み戻し・ランタイムへの画像転送 |
| Subsystems | [`crates/subsystems/audio`](crates/subsystems/audio/AGENT.md) | 音声出力と Steam Audio による立体化 |
| Subsystems | [`crates/subsystems/keyboard`](crates/subsystems/keyboard/AGENT.md) | 仮想キーボードの配列データ・文字・トグル状態 |
| Subsystems | [`crates/subsystems/desktop`](crates/subsystems/desktop/AGENT.md) | 画面・ウィンドウの配信元・画像取得・OS タッチ注入 |
| Subsystems | [`crates/subsystems/media`](crates/subsystems/media/AGENT.md) | 再生中のメディアの情報と操作 |
| 4 | [`crates/overlay`](crates/overlay/AGENT.md) | パネル配置・取り付け・ヒット判定・掴み移動・親子関係・暗転 |
| カーネル | [`crates/kernel`](crates/kernel/AGENT.md) | 設定・フレーム進行・パネル操作を束ねる。UI・画面はカーネルだけを見る |
| 5 | [`crates/ui`](crates/ui/AGENT.md) | パネルを GPUI の非表示ウィンドウで描き、ポインタをマウス入力へ変換する |
| 5 横 | [`crates/design`](crates/design/AGENT.md) | UI の見た目（Constants → Utilities → Components） |
| 5 横 | [`crates/profiler`](crates/profiler/AGENT.md) | 計測中だけ動くプロファイラ |
| 5 横 | [`crates/i18n`](crates/i18n/AGENT.md) | 画面の文言の多言語対応（英語・日本語） |
| 6 | [`crates/testing`](crates/testing/AGENT.md) | テスト用の画面（Testing ほか） |
| 6 | [`crates/wrist`](crates/wrist/AGENT.md) | 常駐の腕時計パネル |
| 6 | [`crates/dashboard`](crates/dashboard/AGENT.md) | ダッシュボード（メニュー・配信・再生中・クイック設定・ランチャー） |
| 6 | [`crates/beam`](crates/beam/AGENT.md) | コントローラのレーザー・カーソル・ドラッグ・ツールチップ |
| 6 横 | [`crates/nyapp`](crates/nyapp/AGENT.md) | ニャプリ（wasm アプリ）の基盤 |
| - | [`crates/nyapp-sdk`](crates/nyapp-sdk/AGENT.md) | ニャプリを書く SDK |
| - | [`crates/testkit`](crates/testkit/AGENT.md) | Playwright 風 E2E テスト基盤（開発専用） |
| - | `src/main.rs` | ブートローダー。ロガーを組み立て、設定を読み、カーネルを起こして載せる画面（wrist とビーム）とともに `ui::run` に渡すだけ。`--novr` / `--openxr` / `--openvr`、指定なしで VR が無ければ novr |

## ドキュメントの置き方（ルール）

- このファイルは**目次と全体の規約だけ**を持つ。クレートの責務・公開 API・設計・テスト手順は `crates/<名前>/AGENT.md`（`subsystems` の下は `crates/subsystems/<名前>/AGENT.md`）に書く。
- クレートごとの `AGENT.md` の形: 先頭に層と責務、続けて `## 主な公開 API`、その後にそのクレート固有の節（設計思想・テスト・未実装）。
- 複数のクレートにまたがる話（依存の向き・フレームの流れ・パフォーマンス・規約）だけがこのファイルに残る。1 つのクレートに閉じる話をここに足さない。
- 新しいクレートを足したら、`AGENT.md` を作り、上の目次に 1 行足す。公開 API を変えたらそのクレートの `AGENT.md` を直す。
- 他のクレートの話は書き写さず、`../kernel/AGENT.md` のように相対リンクで指す。

重い依存は feature で選ぶ。`vr` の `novr`（wgpu・minifb）/ `openvr` / `openxr` を `subsystems` と `kernel` が同名で転送する。ワークスペースの `vr` / `subsystems` / `kernel` は `default-features = false` で、`kernel` の既定は全部無効（型・`connect`・モックだけ。`overlay` / `kernel` のテストはこれで足りる）。実際に起動する `nyaw` と、エミュレーターや実ランタイムを使う `ui` / `testkit` のテストだけが有効にする。無効なランタイムへの接続は `VrError::NoRuntime`。`vr` / `subsystems` 単体の既定は全部有効。

依存は**必ず下向き**。カーネル内の最下部が Subsystems（`os` / `vr` / `input` / `rendering`）で、その上に設定・フレーム進行・パネル管理を置く。依存の入口は `subsystems ← overlay ← kernel ← ui ← 画面 ← main`。`kernel` は設定やフレーム進行のため `subsystems` にも依存する。Subsystems 内は `os ← vr ← input / rendering` で、上位層への依存は禁止する。`audio` は他のサブシステムに依存しない（座標は glam）。
描画に必要な OS 分岐は `rendering` に集約する（`gpui` フィーチャー。`kernel/rendering-host` → `subsystems/rendering-host` で有効にし、下位だけのテストではホスト依存を不要にする）。入力変換・画面の構築とイベントループの進行は UI が担う。ランタイム固有の Vulkan 転送・novr 合成は `vr` に残り、`rendering` が `VrBackend` を通して利用する。
UI・画面・testkit が見る下位の型は `kernel` の再エクスポート経由で、`vr` / `overlay` を直接組み立てない。overlay は `vr` の型（`Pose` 等）を共通語彙として使ってよい。
`logger` は例外で、どの層からも直接依存してよい唯一の横断クレート（`logger` 自身は他の自前クレートに依存しない）。
`design` の値（Constants）と動き（`animation` クレート。`design::animation` で再エクスポート）は GPUI を持たない（`gpui` フィーチャーを外した `design`）ので、`overlay` がパネルの表示アニメーションと影の余白に使ってよい。部品・テーマのグローバルなど GPUI のものは UI 層から上だけ。
`kernel` は `subsystems` を再エクスポートしているが、上位が使ってよいのは型（語彙）とデータ（配列の表など）まで。OS・ランタイムへの副作用（入力の注入・送信）は `PanelCommand` でカーネルに頼む（画面からカーネル本体には届かないので、`PanelControl` に積む）。

## ログ

- ログは**必ず** `logger` のマクロで出す。`println!` / `eprintln!` は使わない（テストハーネスの出力は除く）。
  ```rust
  logger::warn!("パネルの描画を取得できない", error = e, panel = ?panel); // key = Display, key = ?Debug
  ```
- 出力は 1 行 1 件の logfmt: `ts=2026-10-13T09:00:00.123Z level=warn target=ui msg=パネルの描画を取得できない error="device lost" panel=0`。
  `ts level target msg` は固定、以降はフィールドを書いた順。キーは snake_case。`target` は `module_path!()`。
- 出力先は `main` だけが組み立てる: 標準エラー出力（`logger::StderrSink`）と `os::log_dir()/nyaw.log`（`logger::FileSink`。Windows `%LOCALAPPDATA%\nyaw\logs`）。
  起動ごとに新しく作り、前回分は `nyaw-prev.log` に 1 世代だけ残す（既存の prev は消してから退避）。
  ファイル出力・世代管理は `logger::FileSink` / `logger::create_rotated`。保存先の OS 差は `os::log_dir` に閉じ、ローテーションの方針は logger が持つ。logger は自前クレートに依存しない。
- レベルの環境変数は `kernel::config::log_level()` が読む。レベルは環境変数 `NYAW_LOG`（`trace`/`debug`/`info`/`warn`/`error`、既定 `info`）。`init` 前のログは stderr・info 以上に出る。
- 外部クレートの `log` 出力（gpui 等）も同じロガーに流れる。パニックも `main` のフックで `error` として記録する。

## ランタイム（設計思想）

novr・SteamVR (OpenVR)・OpenXR はどれも対等な「手段」でしかない。novr はテスト用の抜け道ではなく、
ランタイム層（`crates/subsystems/vr`）に OpenVR / OpenXR と並んで常に存在し、ビルドしたバイナリでも常に使える。

- **基本操作はどのランタイムでもネイティブに行える**こと。VR 固有の機能（手・コントローラ機種・パネルを掴んで動かす等）
  が novr で使えないのは構わない。
- **上位層はランタイムで分岐しない**。どのランタイムでもパネルは非表示ウィンドウ（GPUI。実質ヘッドレス）で描き、
  読み戻してオーバーレイへ送る。入力はコントローラのレーザー。

novr（エミュレーター）の詳細は [`crates/subsystems/vr`](crates/subsystems/vr/AGENT.md)。

## 1 フレームの流れ（`ui::run`。どのランタイムでも同じ）

```
ui::run（周期は poll が作る: VR はランタイムの vsync、novr は約 90Hz）/ testkit の VrKit::step・NoVrKit::step
 └ ui::pump → kernel.tick()
     ├ subsystems.poll()    → 共通 Frame { hmd, controllers: Vec<ControllerState>, trackers, quit }
     │   ├ backend.poll()  → 生入力（Subsystems 内部だけで扱う）
     │   └ input::map      → 機種別変換・手ごとのヒステリシス（切断・機種変更で履歴を破棄）
     ├ overlay.tick_frame  → 共通フレームからパネル操作を処理
     ├ 取り付け中のパネル   → 姿勢 = frame.pose(device) * offset（掴み中は手に従い、離すとオフセットを付け直す）
     ├ レーザー × パネル判定 → Vec<UiEvent { panel, hand, PointerEvent }>
     ├ ポインタのパネル     → レーザーをヒット点まで伸ばし、カーソルをヒット点へ（無ければ隠す）
     └ vr.set_overlay_pose  （掴み移動を反映）
 └ 手・パネルごとの PointerBridge::translate → gpui::PlatformInput → 対象パネルの window.dispatch_event
 └ ui::sync_panel           → ビューが PanelControl の PanelId ごとに積んだ操作（取り付け・取り外し・ロック）を kernel に反映し、状態を書き戻す
 └ PanelControl::spawns     → 画面から要求された新しいパネルとウィンドウを開く
 └ PanelControl::quit       → アプリを終える（テストのハーネスもこれで終える）
 └ ui::capture(window)      → Frame（RGBA、デバイスピクセル）   testkit は testkit::raster::capture
                              本番は入力・状態の変化があったとき、または 250ms ごとだけ描く（GPU の完了待ちが重い）。1x1 は送らない
 └ overlay.submit(vr, ...)  → vr.submit_overlay_pixels
```

パネル用ウィンドウは `ui::open_panel_window(cx, surface, ..)` で開く。`Overlay` では非表示で、
`Window::render_to_image` で画面に出さずに描画して読み戻す。ルートビューを借用したまま描画・入力すると
二重借用で落ちるので、ウィンドウ操作は `WindowHandle::update` ではなく `cx.update_window(handle.into(), ..)` を使う。

## 画面（UI 層の上。UI 層 1 : 画面 n）

UI 層（`crates/ui`）は1つで、どの画面かを知らない。画面はその上に何個でも載る別クレートで、`ui::run(kernel, Vec<PanelSpec>, profiler)` /
testkit のキットに `build` として渡す。画面からパネルへの操作は GPUI のグローバル `PanelControl` に PanelId ごとに `PanelCommand`
（取り付け・取り外し・ロック）を積み、フレームループが `sync_panel` で反映して `PanelStatus`（ランタイム・取り付け先・ロック・
見えているトラッカー・メニューボタン・掴み中）を書き戻す。全体の設定（テーマ・言語・ポインタ）はパネルを問わないので
`PanelControl::setting_changes` に `Setting` を積み、`sync_settings` が反映して `PanelControl::settings` に書き戻す。画面の型に依存しないので、本番・どのキットでも同じ経路。
新しい画面 = `crates/<名前>` を足して `ui` に依存し、`main` で選ぶ。

## パフォーマンス（設計思想）

オーバーレイは常に目の前にあり、操作はシームレスでなければならない。フレームループ（メインスレッド）が 1 フレームでも
詰まると、レーザー・カーソル・パネルの追従がその場で引っ掛かる。新しい処理を足すときは次を守る。

- **メインスレッドを止めない**。読み込み・展開・デコード・ファイル入出力・重い計算はフレームループの中でしない。
  別スレッドで行い、結果はチャンネル等で次のフレーム以降に受け取る（非同期）。間に合わないフレームは前の結果のまま進める。
  - GPUI のウィンドウ操作・描画のように、メインスレッドでしかできないものは仕方ない。代わりに回数を減らす
    （何もしていないパネルの描き直しは 1 フレームに 1 枚まで・絵が変わらなければ間隔を延ばす、同じ位置への Move は送らない）。
- **分けられる処理は並列にする**。画素ごと・行ごと・パネルごとに独立な処理は、スレッドに分けて同時に進める。
  - 例: novr のエミュレーターの合成は、画面を横帯に分けて帯ごとに別のスレッドで塗る（`std::thread::scope`。目の前のパネルで 5.5ms → 1.3ms）。
- **待ちは正確に**。フレームの周期を作る短い待ちは丸められないものを使う（GPUI のタイマーは Windows では 1ms が 6〜12ms になる。
  フレームループはバックグラウンドのスレッドで眠って譲る。`os::precise_timers` でタイマー精度も 1ms にする）。
- **測ってから直す**。推測ではなく、ストレステスト（`cargo test -p ui --test stress -- --ignored`）とプロファイラ（`--profile`）の
  段階ごとの時間を見る。直したらストレステストの数字で効果を確かめ、コミットに書く。
- 依存クレートのデバッグビルドの罠に注意する。gpui（Windows）は debug-assertions が有効だとウィンドウを作るたびに HLSL を実行時に
  コンパイルする（1 枚 180ms）ので、`Cargo.toml` の `[profile.dev.package.gpui_windows]` で切っている。

## ニャプリ

nyaw の上で動くアプリ。システム UI（wrist・ダッシュボード・ランチャー・デバッグ・設定）もニャプリで、本体に組み込まれた特権のもの（Android の SystemUI と同じ）。ユーザーのニャプリは `.nyapp`（wasm）で、サンドボックスで動き、落ちても nyaw は落ちない。構想・思想・形式は `crates/nyapp/AGENT.md`、試作は `nyapps/`（別のワークスペース。`cargo nyapps build | install`）。

## 規約

- 語彙は OpenXR に合わせる: プロファイル `"valve/index_controller"`、入力 `"trigger/value"`。OpenVR バックエンドが翻訳する。
- 座標: トラッキング空間はメートル・前方 -Z。パネル表面はローカル +Z。パネルのピクセル座標は左上原点。
- 新しいコントローラ対応 = `crates/subsystems/input` の `PROFILES` に 1 行追加。上位層でプロファイル名を見ない。
- 新しいランタイム = `crates/subsystems/vr` に `VrBackend` 実装を追加し `connect` に登録する。
- `CODE_OF_CONDUCT.md` に従う: コメントは日本語、Conventional Commits、CalVer（現在 `2026.0.1`、`[workspace.package]` で一括管理）、単体テスト必須、層をまたぐものは結合テスト。
- **コメントは 1 行で収める**。どうしても長く書く必要があるときも 3 行まで（`///` `//!` `//` とも、連続した 1 かたまりで数える）。それ以上は、コードを分かりやすくするか、設計の話をそのクレートの `AGENT.md` に書いて、コメントからは指すだけにする。lint は置かず、レビューで見る。
- `// ponytail:` コメントは意図的な簡略化。限界と次の一手が書いてある。
- **ナビゲーションバー（厳守）**: 新しいパネルを開くときは、ナビゲーションバーを付けるか（既定）`PanelSpec::without_nav_bar()` で外すかを `crates/design/GUIDELINES.md` の「ナビゲーションバー」に従って必ず判断する。付けるなら `.titled("…")` でタイトルを渡し、パネルの中に閉じるボタン・同じ見出しを埋め込まない。testkit のキットも本番と同じく持ち主の直後（PanelId = 持ち主 + 1）にバーを開く（キットが直接作る最初のパネルには付かない）。

- `gpui` は crates.io 版では `Window::dispatch_event` を外部から呼べないため、Zed の git リビジョンに固定している。`render_to_image` のため `rendering` は本番でも gpui の `test-support` フィーチャーを有効にしている。

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
  - ベンチマーク・長押しの輪・スワイプバック・実 GPU の読み戻しなど `ui` のテストは [`crates/ui/AGENT.md`](crates/ui/AGENT.md)、実ランタイムでの E2E は [`crates/testkit/AGENT.md`](crates/testkit/AGENT.md)。

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
