# crates/kernel

層: 3 カーネル

設定・フレーム進行・パネル操作を束ねる。最下部の Subsystems の上に位置し、UI・画面はカーネルだけを見る。設定は `subsystems::os::config_dir()/config.toml`（アプリ全体の設定はパネルの操作 `PanelCommand` と分け、`Kernel::set(Setting)` で変えて `Kernel::settings()` で読む。`theme = "dark" | "light"` は `Setting::Theme`、`[personal_color]`（`lightness` / `chroma` / `hue`。ボタンなどの塗りに使うユーザーの色。省略で既定の色）は `Setting::PersonalColor`（`Settings::personal_color`。選ぶまで `None`。`ui::sync_settings` が `design::Accent` のグローバルに反映する。色の使われ方は `../design/AGENT.md` の「パーソナルカラー」）、`[[color_history]]`（自分で作ったパーソナルカラーの履歴。新しい順で `COLOR_HISTORY` = 8 個まで。省略で空）は `Setting::CustomColor`（作った色をパーソナルカラーにし、履歴の先頭に覚える。同じ色は 1 つだけ残り、古いものから落ちる。`Settings::color_history`。見本を選ぶだけの `Setting::PersonalColor` では変わらない。選ぶ画面は、この後ろを用意した色で埋めて並べる）、`wrist_design = "dots" | "lcd" | "minimal" | …`（wrist の文字盤のデザイン。既定 `dots`。種類 `WristFace` ごとのデザインの表は `config.rs` の `wrist_designs!`。意味と足し方は `../wrist/AGENT.md`）は `Setting::WristDesign`、`language = "en" | "ja"`（省略なら `Kernel::boot` が OS の表示言語 `os::ui_language` で始める。対応していない言語なら `en`。選ぶまで設定ファイルには書かない）は `Setting::Language`、`pointer_smoothing`（レーザーの手ブレ抑え。0〜0.998、既定 `overlay::POINTER_SMOOTHING` = 上限の 0.998）は `Setting::PointerSmoothing`、`pointer_release_deg`（抑えたレーザーがこの角度だけ手から遅れると抑えが半分になる。0.5〜45°、既定 11.25°）は `Setting::PointerRelease`、`laser_length`（レーザーの見える長さ m。0.05〜1.5、既定 0.3）は `Setting::LaserLength` で切り替えると書き戻され、次回起動に残る。`mirror_frame_rate`（ミラーリングの取得の上限 FPS。1〜240、既定 `desktop::DEFAULT_FRAME_RATE` = 60）は `Setting::MirrorFrameRate`（配信中のミラーもかけ直す。ダッシュボードは配信を始めるときにこの値を `PanelCommand::Stream` に渡す）、`keyboard = "full" | "tenkeyless" | "compact"`（仮想キーボードの種類。既定 `full`）は `Setting::Keyboard`（`KeyboardView` が描き直しで従う）、`block_input = true | false`（ダッシュボードが出ている間、ゲームへの入力を止めるか。既定 `true`。`Settings::block_input`、`Setting::BlockInput`。`Overlay::set_block_input` へ渡し、出ている最中の切り替えも次の tick から効く）, `dominant_hand = "right" | "left"`（利き手。既定 `right`。`Settings::dominant_hand`。`DominantHand::{hand, off_hand}`）は `Setting::DominantHand`: 主に操作する手。カーネルは値を持つだけで、左右で振る舞いが変わる機能がそれぞれ読んで従う（カーネルがパネルを付け替えることはしない）。カーネルの中では、配信のパネルの OS のマウスを利き手だけが動かす（両手で取り合わない。もう片方の手で一度押すと操作する手が替わり、その押下は送らない。OS のカーソルの絵も操作している手にだけ出し、もう片方は nyaw のカーソル）。`ui::sync_settings` が `design::Mode` のグローバルに反映し、画面は `Theme::current(cx)` で引く）。OS への入力（配信のタッチ・仮想キーボードのキー）はどちらもカーネル経由（`PanelCommand::Stream` / `PanelCommand::TypeToOs`）。優先順位はコマンドライン > 設定ファイル（ログレベルは `NYAW_LOG` > 設定ファイル）

## 主な公開 API

`Kernel::{boot, with_backend, tick, submit, apply, status, set, settings, os_keyboard, add_panel, batteries, now_playing}`（メディアの見張りは `boot` だけが起こす）, `PanelCommand`（`Hide(bool)` で閉じずに隠す・出し直す、`Resizable(bool)`、`GrabAnchor(Option<Pose>)`、`Media(media::Command)`、`FocusSource`（配信しているウィンドウを OS で前に出す））, `PanelStatus`（`hidden`）, `Setting`, `Settings`, `config::{load, parse, path, Config}`, `subsystems`, `rendering` / `vr` / `overlay` の再エクスポート

## パネルの LOD

- `config.toml` の `lod_quality`: 0 = Off（常に LOD0）、1 = Low、2 = Mid（既定）、3 = High。`Kernel::panel_lod` が HMD とパネル中心の距離から決める。距離はパネルの密度（1m あたりの論理ピクセル）で換算し、基準（1000px/m）より粗いパネルは近くにあるものとして扱う（拡大すると LOD が下がり、ぼやけたまま残らない。Issue #51）。
- LOD0〜3 は縦横 1 / 1/2 / 1/4 / 1/8。切り替え距離は Low が 1 / 2 / 4 m、Mid が 2 / 4 / 8 m、High が 4 / 8 / 16 m。
- フレームレートも LOD で絞る。LOD0 は毎フレーム、LOD1〜3 は 2 / 4 / 8 フレームに 1 回だけ描き直す（`Kernel::panel_frame_due`。パネルごとにずらす）。間引いたフレームの入力・変化は次に描いてよいフレームで描く。
- GPUI は非表示ウィンドウの DPI と実ピクセルサイズを変える。カーネルの論理解像度と物理サイズは変えない。8 の倍数でないサイズは表示領域に最大 7 論理ピクセルの丸めがある。
- `cargo test -p ui --test gpu -- --ignored panel_lod` で実描画と入力を確認し、`--off` を追加して無効時も確認する（Windows）。
- ストレステストは `NYAW_STRESS_LOD=0` と `2` で同じ負荷を比較できる。

## ロックダウン

パネルが取る・放す汎用の仕組み（`PanelCommand::Lockdown(bool)`。`PanelSpec::lockdown()` で開いた時点から取る）。特定の画面のためのものではない。

- **取り手**: パネルごとに取り、何枚でもよい。全員が放す（か閉じる）までロックダウン（`Kernel::lockdown` / `Settings::lockdown`）は続く。設定ファイルには残さない。
- **守るもの**: 取っているパネルは、自分が放すまで閉じられない（`PanelCommand::Close` を無視する）。取っていないパネルには影響しない。
- **シェルの振る舞い**: カーネルは状態を渡すだけで、何を控えるかは見る側（シェル）が決める。今は wrist が、ロックダウンに入った瞬間に自分自身と出ているダッシュボードを隠し（解けると手首は出し直し、ダッシュボードは出し直さない）、中は二回押しを無視して開閉しない。
- **放して閉じる**: 閉じる前に放すときは、同じパネルのコマンドに `Lockdown(false)`、`Close` の順で積む（同じパネルのコマンドは積んだ順に処理される）。

`Setting::SetupDone(bool)`（`setup_done`）はセットアップウィザードを終えたか。省略で `false`。
