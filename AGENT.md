# AGENT.md

nyaw は GPUI で GUI を描く VR オーバーレイシステム。Cargo ワークスペースで、層ごとに 1 クレート。

## 層構成（下 → 上）

| 層 | クレート | 責務 | 主な公開 API |
|---|---|---|---|
| - | `crates/logger` | logfmt 形式の構造化ロガー（横断）。整形・レベル・出力先 `Sink` の抽象と、`log` クレート（gpui 等）からの橋渡し | `trace!/debug!/info!/warn!/error!`, `Logger`, `Sink`, `StderrSink`, `FileSink`, `create_rotated`, `init`, `format` |
| カーネル最下部 / Subsystems | `crates/subsystems` | OS・ランタイム・コントローラ入力・描画の差を吸収。ランタイムと入力履歴を所有し、上位には生入力のない共通フレームを渡す。設定・パネル管理・UI には依存しない | `Subsystems::{with_backend, poll}`, `Frame`, `ControllerState`, `os`, `rendering` |
| カーネル / Subsystems / OS | `crates/subsystems/os` | Windows/Linux の差を吸収（保存先、ランタイム検出、タイマー精度・GPU 優先度）。OS の一般的な `#[cfg]` はここに書く（描画固有は `rendering`） | `config_dir`, `log_dir`, `precise_timers`, `high_gpu_priority`, `openvr_paths_file`, `openxr_runtime_manifest` |
| カーネル / Subsystems / ランタイム | `crates/subsystems/vr` | OpenVR / OpenXR / novr の差を吸収。`Subsystems` が `VrBackend` を所有し、上位は共通フレームを受け取る。novr はデスクトップのウィンドウで VR を模すエミュレーター | `connect`, `VrBackend`, `Runtime`, `novr::{Emulator, Button}`, `Pose`, `Frame`（`pose(&Device)`。`hands` に骨格、`batteries` に電池の残量 `Battery`（OpenVR は 5 秒ごとに読み直す。OpenXR は標準の手段が無く空、novr は固定の模擬値））, `Device`, `RawController`, `HandSkeleton`（スケルタル入力。関節 `JOINT_COUNT` = 26 は OpenXR の並び、`joint_parent`, `TIPS`, `pinch_distance`。OpenXR は XR_EXT_hand_tracking、OpenVR は骨格のアクション。novr には無い）, `mock::MockBackend`（`test-support` フィーチャー時のみ） |
| カーネル / Subsystems / 入力 | `crates/subsystems/input` | コントローラ機種の差を吸収（ドライバ的役割）。プロファイル表で生入力 → 抽象アクション。コントローラを持たない手は、骨格から作った仮想コントローラ（`hand_controller`、プロファイル `ext/hand_interaction_ext`）で操作する: 親指と人差し指のつまみ = クリック、中指・薬指・小指を握る = 掴む、左手の手のひらを顔に向けて親指と小指を `kernel::HAND_LONG_PRESS`（0.5 秒）付け続ける = ショートカット（X ボタンの二回押しと同じく `PanelStatus::double_press` に数える）。握りは指の曲がりの角度。レーザーの元は親指と人差し指の付け根の間、向きは手の向き（手首から中指の付け根へ。`HandSkeleton::grip`。手首を曲げれば動き、指では動かない）。コントローラに取り付けたパネル（wrist）は、骨格の手でもランタイムがその手のデバイスを追っていれば（`vr::Frame::hand_devices`。OpenVR は入力のアクションか手の役割 `GetTrackedDeviceIndexForControllerRole` のデバイス。Virtual Desktop のハンドトラッキングは `vd_hand_controller`）コントローラと同じく相対配置で追従させ、無ければ `grip`（揺れを `input::PoseFilter` で抑えたもの。元は手首の時計が腕時計の位置に来る所）に付く。骨格とコントローラの両方がある手は、入力の元の切り替えを 18 フレーム続いてから行う（持ち替え中の申告の揺れでクリックが効かなくならないように）。ただしコントローラが一瞬見えずに骨格へ落ちた手は、戻ればすぐコントローラに戻す。OpenVR は相対配置を任せたオーバーレイの取り付け先の裏のデバイスが入れ替わると付け直す。レーザーは `HandFilter`（One Euro 風。止めると強く抑え、速く振ると付いてくる。つまみかけ〜押している間はさらに鈍くし、クリックしている間は `click_hold_deg` 以内の動きでは押した瞬間のレーザーのまま止める）で揺れを抑える。指を全部追っている手（`vr::FingerTracking::Full`。OpenVR の `GetSkeletalTrackingLevel`。光学式のハンドトラッキングをランタイムがコントローラとして見せていても）も骨格で操作する。しきい値と抑えは `HandTuning`（`config.toml` の `[hand]`、`Setting::Hand`。Subsystems の `hand`）。判定に使った値は `ControllerState::metrics` と `NYAW_LOG=debug` の `controller buttons` に出る。`ControllerState::facing` に手のひら / 手の甲（骨格の手だけ）。Subsystems が `poll` で足す | `map(&RawController, prev) -> ControllerState`, `source(profile, Action)` / `scroll_source(profile)`（逆引き） |
| カーネル / Subsystems / 描画 | `crates/subsystems/rendering` | GPUI の非表示描画先・読み戻し・描画密度と OS 差、ランタイムへの画像転送・不透明度 / 相対追従の能力差・描画資源の保持と解放。UI は `kernel::rendering` 経由で使う | `Surface::{new, store, present(opacity, dim), place, show, release}`, `gpui::{capture, read_back, open_window, set_render_lod}` |
| カーネル / Subsystems / 音声 | `crates/subsystems/audio` | OS の音声出力（cpal: Windows は WASAPI、Linux は ALSA）・WAV の読み込み・Steam Audio による立体化（HRTF）。出力・ミックス・立体化は別スレッドで、メインスレッドは命令を送るだけ。音は「スピーカー」（音源）から鳴る | `Engine::{open, offline, render, set_listener, set_speaker, remove_speaker, play, play_sound, stop, stop_speaker, poll, playing, spatializer}`, `Speaker`, `Layout::{Auto, Mono, Stereo}`, `Channels`, `Clip`, `Sound::{File, Wav}`, `Spatializer` |
| カーネル / Subsystems / キーボード | `crates/subsystems/keyboard` | 仮想キーボードの配列データ（xkeyboard-config）・文字・トグル状態。OS には触らない（OS への入力は `os::keyboard`） | `Database`, `Keyboard`, `Layout`, `Stroke`, `KeyboardType` |
| カーネル / Subsystems / 配信 | `crates/subsystems/desktop` | 画面・ウィンドウの取得元の一覧・画像取得・OS タッチ注入（「画面・ウィンドウ配信」） | `Sources`, `Stream`, `Source`（ウィンドウは `icon` にアプリのアイコン。Windows はウィンドウのもの、無ければ実行ファイルのもの）, `Icon`, `Status`, `Touch` |
| 4 オーバーレイ | `crates/overlay` | パネル配置、デバイスへの取り付け（追従）・掴めないようにするロック（空間固定）、レーザーのヒット判定、ポインタイベント生成、パネルの掴み移動、親子関係（子は親に追従、子が開いている間の親は暗くなり（子のフェードと同じ時間で暗転・復帰）入力を受けない、親が閉じれば子も閉じる）、ナビゲーションバー（持ち主の地の下端の左右中央に、バーの上端の左右中央を合わせて毎 tick 置く。頭からの距離に比例して大きくし見た目の大きさを保つ。持ち主と一緒に閉じ・暗くなり、掴むと持ち主を掴む）、空間全体の暗転（`set_veil` したパネルが出ている間、黒い幕を毎 tick 頭の正面 50m（描画順 0。パネルは 1 から。描画順でも奥行きでもパネルより奥）に置き、パネルの出入りと同じ動きで暗転・復帰する。暗転している間はゲームへの入力も止める（`VrBackend::block_app_input`。OpenVR は SteamVR Input のアクションセットをオーバーレイの全体優先度で更新して入力を奪う（SteamVR のメニューが開いている間は奪わない）。この仕組みは SteamVR の開発者向け設定「Experimental overlay input overrides」（`steamvr/globalActionSetPriority`）が無効だと働かないので、最初に止めるときに無効なら有効にする。OpenXR・novr は何もしない）） （パネルの表示アニメーションと影の余白は `design` の GPUI を持たない部分＝値と `design::animation` から引く。`default-features = false`） | `Overlay::{add_panel, attach, detach, set_locked, set_parent, set_hidden, set_nav_bar, set_veil, set_resolution, tick, submit}`, `Parent { panel, carry, dismiss, hud, docked, slot }`（`dismiss` = 子の外側を押すと子を閉じる。ブロックしたい子だけ `PanelSpec::without_dismiss`。`docked` = 親に固定した子で、親を暗くせず入力も止めない。`slot` = 親の上端の上に、子のナビゲーションバーの分を空けて、親から見て指定の角度だけ起こして立てる（子の大きさが変わっても毎 tick 置き直す）。`slot` にはめる・外すときは `Motion::Move` で滑らかに動き、外すと離れたと分かるよう面の向きへ 0.2m 手前に出る。固定した組（バー・はめた画面・キーボード）はどれを掴んでも上にはめた画面を掴んだことにし（頭に向けて垂直に立て、ほかは付いてくる。はめた画面が無ければ根元の `grab_anchor`（ダッシュボードは画面が立つ所）を基準にする）、はめた画面は下端を動かさずに大きさを変える。固定した子のナビゲーションバーは頭からの距離で大きさを変えない）, `Overlay::set_resizable`（掴んで大きさを変えられるか。掴んだものと動かすものが違うときも変えない）, `Overlay::set_grab_anchor`（掴んだとき手で動かして頭へ向ける基準。`PanelSpec::grab_anchor`）, `Overlay::set_hidden`（閉じずに隠す。フェードアウトし、入力・暗転の対象外。子も一緒に隠れる）, `Panel::pixel_to_world`, `UiEvent`, `PointerEvent` |
| カーネル | `crates/kernel` | 設定・フレーム進行・パネル操作を束ねる。最下部の Subsystems の上に位置し、UI・画面はカーネルだけを見る。設定は `subsystems::os::config_dir()/config.toml`（アプリ全体の設定はパネルの操作 `PanelCommand` と分け、`Kernel::set(Setting)` で変えて `Kernel::settings()` で読む。`theme = "dark" | "light"` は `Setting::Theme`、`language = "en" | "ja"` は `Setting::Language`、`pointer_smoothing`（レーザーの手ブレ抑え。0〜0.998、既定 `overlay::POINTER_SMOOTHING` = 上限の 0.998）は `Setting::PointerSmoothing`、`pointer_release_deg`（抑えたレーザーがこの角度だけ手から遅れると抑えが半分になる。0.5〜45°、既定 11.25°）は `Setting::PointerRelease`、`laser_length`（レーザーの見える長さ m。0.05〜1.5、既定 0.3）は `Setting::LaserLength` で切り替えると書き戻され、次回起動に残る。`ui::sync_settings` が `design::Mode` のグローバルに反映し、画面は `Theme::current(cx)` で引く）。OS への入力（配信のタッチ・仮想キーボードのキー）はどちらもカーネル経由（`PanelCommand::Stream` / `PanelCommand::TypeToOs`）。優先順位はコマンドライン > 設定ファイル（ログレベルは `NYAW_LOG` > 設定ファイル） | `Kernel::{boot, with_backend, tick, submit, apply, status, set, settings, os_keyboard, add_panel, batteries}`, `PanelCommand`（`Hide(bool)` で閉じずに隠す・出し直す、`Resizable(bool)`、`GrabAnchor(Option<Pose>)`）, `PanelStatus`（`hidden`）, `Setting`, `Settings`, `config::{load, parse, path, Config}`, `subsystems`, `rendering` / `vr` / `overlay` の再エクスポート |
| 5 UI | `crates/ui` | パネルを GPUI の非表示ウィンドウで描き、ポインタイベントをマウス入力へ変換する。1 つのフレームループ・カーネルで回し、描画結果を `kernel::rendering` 経由でパネルへ返す。画面（アプリ）の種類は知らない。どのパネルにも付くシステムの部品（ナビゲーションバー・仮想キーボード `ui::keyboard`・プロファイラパネル）だけは UI 層が持つ | `run(kernel, panels, profiler)` / `run_with`（テスト用の `Hooks`）, `PanelControl::batteries`（毎フレーム書く）, `PanelSpec::{new, spawn, child, below, hinged_below, docked_to, dockable, fit_height, without_resize, titled, titled_text, without_nav_bar, with_os_keyboard, with_veil, nav_bar_spec}`, 画面をはめる場所 `PanelControl::dock`（ダッシュボードが開くと置く）と `dock` / `undock` / `docked`（はめると持ち主に `Parent { docked, slot }` で固定し、はまっていた画面は閉じる。`dockable` のパネルはナビゲーションバーの左端に外す `nav-undock` / 戻す `nav-dock` を出す。はめた画面へ打つ仮想キーボードはバーの下端に蝶番のようにつなげ、水平から手前が 10° 下がる向きに寝かせてダッシュボードに固定する（`PanelSpec::hinged_below` + `docked_to`。画面・バー・キーボードがつながって見える。画面を外すとキーボードも外れて独立する）。ほかのパネルから出すキーボードは真下に出して頭（目と手の間）へ向ける（`PanelSpec::below`））（ナビゲーションバーは既定で付き、開く側が持ち主の直後に開く。ポインタのパネルには付かない）, `pump`, `sync_panel`, `sync_settings`, `sync_tooltips`, `capture`, `open_panel_window`, `drag_preview`, `PointerBridge`, `PanelControl`, `PanelState::live`, `PanelCommand`, `PanelStatus`, `MAX_PANELS` |
| 5 横 | `crates/design` | UI の見た目の責務。Radix Colors のエイリアスに倣う 3 層。Constants（`scale::{light,dark}::SLATE_1..12` / `BLUE_*` / `RED_*`、`scale::{WHITE,BLACK}`、`radius` / `spacing` / `font_size` / `letter_spacing` / `duration_ms`。段ごとに用途が決まった 12 段の生の値）→ Utilities（`Theme`。`accent` → `blue` のスケールの別名と段の用途名 `accent_solid` など、テーマで指す先が変わる `background`（パネルの地）/ `card` / `control`（ボタンの既定）/ `groove`（スライダーの空き）/ `track` / `on_accent` など。`Mode::{Dark, Light}` と `Theme::of`。部品は役割だけを見る）→ Components（`button` / `switch` / `slider` / `panel` + `card`（カード複数）/ `card_panel`（カード 1 枚）/ `navigation`（画面の積み重ね。右から入る push 遷移と、押したまま右から左へ動かすスワイプバック。タッチ・マウスとも。中の要素が止めた入力は奪わず、縦の一覧は `restrict_scroll_to_axis()` を付ける）/ `dialog`（子パネルの中身: アイコン・タイトル・概要・ボタン）/ `nav_bar`（パネルの下に付くナビゲーションバー: 中央にタイトル、右端に閉じるボタン `nav-close`。持ち主のパネル（かバー）を指していない間は丸い線に縮み、指すと広がる）/ `list` + `list_item`（カード色の行を区切り線で並べる一覧）/ `tile`（地に直接並べる押せるカード。グリッド用。中身は呼び出し側が足す。`Button` は `ParentElement` なので中身を足せる）/ `sidebar_item`（サイドバーの行。背景が無く、選んでいる行だけアクセントの色）/ `surface`）。値は `tokens.json`（色は Radix Colors を OKLCH にしたもの）が唯一の情報源で、build.rs が `palette` クレートで sRGB にしてコードを作り、役割の組が WCAG のコントラスト比を満たさなければビルドを落とす | `icon(&icons::HEART).weight(Weight::Fill).size(..).color(..).rotate(..)`（Phosphor を path で描く。色は `Background` なのでグラデーションでアイコンの形に抜ける。6 ウェイト全対応。**ソースで使われているアイコン・ウェイトだけ**を build.rs が埋め込む。データは `icons/*.tsv`、更新は `icons/vendor.mjs`）, `Theme::{DARK, LIGHT, of}`, `Mode`, `components::{button, icon_button, switch, slider, panel, card, card_panel, dialog, PANEL_MARGIN, surface, tooltip, Tooltips, hover_tooltip, Variant}`（`Variant::{Default, Primary, Danger}`。既定は無彩色）, `scale::*`, `catalog::Catalog`（デバッグ用カタログ。色の段・役割・定数・文字・アイコン・部品・動きを 1 画面で確認する。`cargo run -p design --example catalog`）。見た目の方針は `crates/design/GUIDELINES.md`|
| 5 横 | `crates/profiler` | 計測中だけ動くプロファイラ（`--profile` で起動時から、または Testing の Profiler ボタン / `PanelControl::profiling` で切り替え）。UI 層のフレームループが段階ごとの所要時間（wait / input / draw / render / upload / overlay / compositor と、プロファイラ表示自身の描画 profiler）を記録し、FPS・フレーム時間・CPU/GPU・ドロップをプロファイラパネル（ナビゲーションバーの閉じるボタンで計測ごと止める）にグラフで出す。`--profile` のときは 1 フレーム 1 行の CSV（`os::log_dir()/profile.csv`）にもダンプする。GPU は同期して待つ段階（描画+読み戻し、VR への転送）の実時間 | `Profiler::{new, with_budget, record, add_self_time, summary}`, `ProfilerView`, `Stages`, `CSV_HEADER` |
| 5 横 | `crates/i18n` | 画面の文言の多言語対応（英語・日本語）。言語ごとの表 `locales/{en,ja}.toml`（キーは `.` 区切り、差し込みは `{name}`）を埋め込み、GPUI のグローバル `Lang`（`ui::sync_panel` がカーネルの `language` から反映）で引く。無いキーは英語、それも無ければキー。テストが、全言語に全キーと同じ差し込みがあるか、ソースの `i18n::t(cx, "…")` のキーが表にあるかを確かめる | `t(cx, key)`, `tf(cx, key, &[(name, &value)])`, `Lang::{En, Ja, ALL, native_name, current}`, `get`, `keys`, `missing` |
| 6 画面 | `crates/testing` ほか（n 個） | UI 層の上に載る画面。GPUI のビューで、ランタイムを意識しない | `Testing`（テスト用の画面）, `TestingAnim`（毎フレーム変わる）, `TestingLod`（LOD を数字と色で表示）, `TestingScrollbar`（縦・横・縦横のスクロールバー）, `StressTest`（上限までパネルを出す・閉じる・ちりばめる・並べる）, `TooltipTest`（ツールチップ）, `SmoothingTest`（レーザーの手ブレ抑えの強さと線の長さをスライダーで調整） |
| 6 画面 | `crates/wrist` | 常駐パネル。左手首に腕時計のように取り付き（掴めない）、丸い文字盤（縁に目盛りと秒の点）に日付・時刻。常駐なので、X ボタンの二回連続押し（`PanelStatus::double_press` の回数が増える）でダッシュボードを開く（2 回目からは隠す / 出し直す。閉じない）。ダッシュボードから開くデバッグ画面・設定画面は main が渡す（`wrist::spec(debug, settings)`。今は Testing とその設定画面。画面どうしは直接依存しない） | `wrist::spec(debug, settings)`, `Wrist`, `WATCH_OFFSET` |
| 6 画面 | `crates/dashboard` | ダッシュボード。左手の X ボタン（Touch は X、Index は左手の A、novr はキーボードの X、コントローラの無い手は手のひらを顔に向けて親指と小指を 0.5 秒付け続ける）を `kernel::DOUBLE_PRESS`（250ms）以内に二回押すと開き（カーネルは画面を知らず、左手の X をショートカットボタンとして `PanelStatus::double_press` に数えるだけ。開くのは wrist）、もう一度二回押すと閉じる。開いている間は空間全体が暗くなる（`PanelSpec::with_veil`）。ダッシュボードは少し上を向けたバー（`Dashboard`。掴んで動かせる）そのもので、開いた時は何も出さない。はめた画面が目の高さの正面 1.1m に垂直に立つよう、バーはその下に置く（`dashboard::pose`。オーバーレイの `slot` の置き方の逆算。`overlay::SLOT_GAP` と `ui::NAV_BAR_HEIGHT_M` を使う）。左から時刻と HMD の電池（`dashboard-quick-settings`）・画面のミラーリング（`dashboard-mirror`）、右端にデバッグ（`dashboard-debug`。main が渡す画面。今は Testing）・設定（`dashboard-settings`。main が渡す画面。今は Testing の設定画面）。閉じるボタンは置かない。ボタンを押すとその画面（自分のナビゲーションバーを持つ別のパネル）をバーの上にはめ（`ui::dock`。はまっていた画面は閉じる）、同じボタンをもう一度押すと閉じる。はまっている画面のボタンは強調色。はめた画面はダッシュボードに固定され（一緒に動き、掴むとダッシュボードごと動く）、ナビゲーションバーの `nav-undock` で外して独立させ（手前へ離れる）、`nav-dock` で戻せる。閉じる（X ボタンの二回押し・出ている間にダッシュボードにもほかのパネルにも当たらない所でトリガーを押す（`PanelStatus::outside_press`））と、はめた画面ごと隠すだけ（`dashboard::hide`。配信も止めない）で、次は同じ状態で目の前に出る（`dashboard::show`）。画面（`Page`）: ミラーリング（`Screen`）= 画面（モニターのアイコン）とウィンドウ（アプリのアイコン）のタイル（`dashboard-source-N`）をグリッドで並べ、押すと配信のパネルが一覧と入れ替わりにはまる（OS キーボードはバーの下に固定されて出る。画面を外すと一緒に外れる）。クイック設定（`Screen`）= 上に時刻・日付と設定ボタン（`quick-settings-open`。バーの設定ボタンと同じ）、テーマの切り替え（`quick-theme`）、デバイスの電池のカード（`battery-hmd` / `-left` / `-right` / `-tracker-N`。HMD・左・右・トラッカーの順） | `dashboard::{spec(opened, settings, debug), pose, hide, show}`, `Dashboard`, `Screen`, `Page`, `Summon`, `date`, `OPEN_SOUND`, `CLOSE_SOUND` |
| 6 画面 | `crates/beam` | コントローラのビーム。手ごとにレーザー（線）とカーソル（パネルに貼り付く円）とドラッグの見た目（ドラッグ中だけ。中身は `ui::drag_preview` で包んだビューを GPUI が描き、パネルの外・ウィンドウをまたいでも見える）とツールチップ（カーソルの子。指している要素が `.tooltip(..)` で置いた説明を、頭から見て手の側（`kernel::Anchor`。手が下なら下）にスライドで出す。離れても 0.125 秒は残り、その間に別の要素を指すと文字と幅が変わる）の 4 枚。見た目だけを描き、置き場所・長さ・表示・描画順は `PanelSpec::pointer` で役割を与えたオーバーレイ層が毎 tick 決める（ヒット判定の対象外）。どのランタイムでも載せる（novr のレーザーは視線と重なって見えない） | `beam::specs()`, `Laser`, `Cursor`, `DragLayer`, `Tooltip`（`tooltip::State`） |
| - | `crates/testkit` | Playwright 風 E2E テスト基盤。開発専用。ランタイムごとに対等なキット | `Driver::{click, hover, down, up, drag, scroll, read, color_at}`（全キット共通）, `on_every_runtime!`, `NoVrKit::{screenshot, on_screen, color_on_screen, move_mouse, press, release, stick, select_panel, type_keys}`, `DesktopKit::with_backend`, `VrKit::{panel, add_panel, press, drag_panel, move_hand, hand, profile, point_at}`, `runtimes::{install, launch, Launched::connect}`（実ランタイムの用意・起動・後片付け） |
| - | `src/main.rs` | ブートローダー。ロガーを組み立て、設定を読み、カーネルを起こして（ランタイムの選択はカーネルの仕事）載せる画面（wrist とビーム。wrist が召喚する画面もここで選ぶ）とともに `ui::run` に引き渡すだけ。`--novr` / `--openxr` / `--openvr`、指定なしで VR が無ければ novr | — |

重い依存は feature で選ぶ。`vr` の `novr`（wgpu・minifb）/ `openvr` / `openxr` を `subsystems` と `kernel` が同名で転送する。ワークスペースの `vr` / `subsystems` / `kernel` は `default-features = false` で、`kernel` の既定は全部無効（型・`connect`・モックだけ。`overlay` / `kernel` のテストはこれで足りる）。実際に起動する `nyaw` と、エミュレーターや実ランタイムを使う `ui` / `testkit` のテストだけが有効にする。無効なランタイムへの接続は `VrError::NoRuntime`。`vr` / `subsystems` 単体の既定は全部有効。

依存は**必ず下向き**。カーネル内の最下部が Subsystems（`os` / `vr` / `input` / `rendering`）で、その上に設定・フレーム進行・パネル管理を置く。依存の入口は `subsystems ← overlay ← kernel ← ui ← 画面 ← main`。`kernel` は設定やフレーム進行のため `subsystems` にも依存する。Subsystems 内は `os ← vr ← input / rendering` で、上位層への依存は禁止する。`audio` は他のサブシステムに依存しない（座標は glam）。
描画に必要な OS 分岐は `rendering` に集約する（`gpui` フィーチャー。`kernel/rendering-host` → `subsystems/rendering-host` で有効にし、下位だけのテストではホスト依存を不要にする）。入力変換・画面の構築とイベントループの進行は UI が担う。ランタイム固有の Vulkan 転送・novr 合成は `vr` に残り、`rendering` が `VrBackend` を通して利用する。
UI・画面・testkit が見る下位の型は `kernel` の再エクスポート経由で、`vr` / `overlay` を直接組み立てない。overlay は `vr` の型（`Pose` 等）を共通語彙として使ってよい。
`logger` は例外で、どの層からも直接依存してよい唯一の横断クレート（`logger` 自身は他の自前クレートに依存しない）。
`design` の値（Constants）と動き（`design::animation`）は GPUI を持たない（`gpui` フィーチャーを外した `design`）ので、`overlay` がパネルの表示アニメーションと影の余白に使ってよい。部品・テーマのグローバルなど GPUI のものは UI 層から上だけ。
`kernel` は `subsystems` を再エクスポートしているが、上位が使ってよいのは型（語彙）とデータ（配列の表など）まで。OS・ランタイムへの副作用（入力の注入・送信）は `PanelCommand` でカーネルに頼む（画面からカーネル本体には届かないので、`PanelControl` に積む）。

## ログ

- ログは**必ず** `logger` のマクロで出す。`println!` / `eprintln!` は使わない（テストハーネスの出力は除く）。
  ```rust
  logger::warn!("パネルの描画を取得できない", error = e, panel = ?panel); // key = Display, key = ?Debug
  ```
- 出力は 1 行 1 件の logfmt: `ts=2026-10-13T09:00:00.123Z level=warn target=ui msg=パネルの描画を取得できない error="device lost" panel=0`。
  `ts level target msg` は固定、以降はフィールドを書いた順。キーは snake_case。`target` は `module_path!()`。
- 出力先は `main` だけが組み立てる: 標準エラー出力（`logger::StderrSink`）と `os::log_dir()/nyaw.log`（`logger::FileSink`。Windows `%LOCALAPPDATA%\nyaw\logs`、Linux `$XDG_STATE_HOME/nyaw/logs`）。
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
- **novr は VR のエミュレーター**（`vr::novr::Emulator`）。最初の poll でウィンドウ（`nyaw (novr)`）を開き、受け取ったテクスチャを
  固定した頭（原点・前方 -Z、縦の視野 70°）から見た絵として合成する。重なりは VR と同じく描画順。背景は透過がわかるよう無彩色の市松模様。
  ウィンドウを閉じると終了を求める（`Frame::quit`）。poll は約 90Hz に待って周期を作る（VR の vsync 待ちに当たる）。
  - 合成は GPU（wgpu。`vr::compositor`）でウィンドウに直接描く。使えなければ（または `gpu.emulator = false` か `NYAW_EMULATOR_CPU=1`）CPU で合成して minifb に渡す。
    GPU のときも、見えたか（`VrBackend::overlay_seen`。隠れたパネルの描き直しを止めるカリングに使う）は縦横 1/8 の CPU の合成で求める。
    スクリーンショットはいつも CPU で合成し直す。`Emulator::headless` はウィンドウを出さず CPU で合成し、`Emulator::scripted_window` は
    ウィンドウと合成は本番どおりで入力だけ注入する（ベンチマーク用）。
  - カーソルの周りの輪は長押しの進み（`PanelControl::hold`）。満ちるとマウスの押下になり、その場でドラッグが始まる。
  - 片手操作: 右手のレイは目からマウスの方向（レーザーは視線と重なるので見えず、カーソルだけ見える）。左クリック = トリガー、
    右クリック = グリップ（掴む）、X キー = 左手の X ボタン（`novr::Button::X`）、矢印キー = 右手のスティック、掴んでいる間の PageUp / PageDown = パネルの拡大 / 縮小（テスト用。`vr::RESIZE_INPUT` → `ControllerState::resize`）。左手は手元で下を向いていてパネルを指さず、WASD = 左手のスティック。
  - デバイスは実在しないので取り付けたパネルは追従しない（Subsystems が返す共通フレームの `pose` が `None`。取り付け先を見失ったのと同じ扱い）。
  - キーボードはスティックなのでパネルには届かない（VR と同じ）。
  - Linux は GPUI を headless で動かし（`ui::application`）、窓を出さずに wgpu で描いて読み戻す（vendor/gpui_linux・vendor/gpui_wgpu）。ディスプレイは要らず、GPU はソフトウェア実装（Mesa の lavapipe / llvmpipe）でも動く。
- ビューはランタイムを意識しない（どのランタイムでも同じビューが動く）。

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

### Testing（`crates/testing`。テスト用の画面）

一連の機能を1画面で試せる画面。新しい機能を足したら Testing にも操作を足し、E2E はその操作で書く。有効な状態は `BUTTON_ACTIVE_BG` で光る。

| 区分 | 要素（`debug_selector`） | 試せること |
|---|---|---|
| VR | `click-me` | クリック・ホバー（全機種・左右の手） |
| VR | `spawn-panel` | 隣に Testing パネルを追加し、その画面を独立して操作する |
| VR | `locale-test` | 隣にローカライズを確かめる画面（`LocaleTest`）を開く。`lang-en` / `lang-ja` で言語を切り替え（設定ファイルに残る）、`locale-missing` に訳の抜け、`locale-samples` に長い文・差し込み・ボタン、`locale-table` に全キーを言語ごとに並べる。Testing の `toggle-language` でも次の言語へ切り替わる |
| VR | `tooltip-test` | 隣にツールチップを試す画面（`TooltipTest`）を開く。`tip-short` / `tip-long`（隣り合い、移ると文字と幅が変わる）、`no-tip`（説明なし）、アイコンだけのボタン `tip-icon-*`（説明は必須）、`tip-far`（離れていて猶予が切れる）。`tip-status` に手ごとの文字と向き |
| VR | `pointer-test` | 隣にポインタの方式を試す画面（`PointerTest`）を開く。`pointer-mode` でマウス / タッチのエミュレートを切り替え（タッチで光る）、`drag-chip` → `drop-zone`（マウスのドラッグ）・`swipe-list`（タッチのスワイプ）・`tap-me`（どちらでも押せる） |
| VR | `dialog-test` | 隣に親子関係を試す画面（`DialogTest`）を開く。`open-dialog`（外側を押すと閉じる既定のダイアログ）と `open-modal-dialog`（`without_dismiss`。答えるまで残る）でダイアログ（`Dialog`。`PanelSpec::child(.., carry = true, ..)`）を子として出す。開いている間は親が暗くなって押せず（掴んで動かすと子が付いてくる）、ダイアログを掴むと親も付いてくる（独立しない）。`dialog-ok` / `dialog-cancel` で答えて閉じる |
| VR | `settings-test` | 隣に設定画面（`Settings`。中身はダミー）を開く。左のサイドバーのカテゴリ（`settings-general` など。選んでいるものだけ色が付く）を押すと、区切り線（`settings-separator`）の右に項目の一覧（`setting-0..`）が切り替わる。項目を押すと右から詳しい画面（`setting-detail-0..`）が入り（`components::navigation`）、`settings-back` か押したまま右から左へ動かすと戻る。閉じるのはナビゲーションバー |
| VR | `nav-test` | 隣に画面の積み重ねを試す画面（`NavTest`）を開く。`nav-item-0..` → `nav-deeper` で右から次の画面が入り（push 遷移）、`nav-back` か押したまま右から左へ動かす（スワイプバック。幅の半分を越えるか払うと戻る）で戻る。離した所の `nav-tap` は押されず、`nav-slider` のハンドルは奪わない。パネルを掴んでいる手のポインタは無視する（E2E は `navigation.rs`） |
| VR | `audio-test` | 隣に立体音響を試す画面（`AudioTest`）を開く。`audio-left` / `audio-centre` / `audio-right`（片側だけの 2ch・左右が同じ 2ch）と `audio-loop-1..4`（2ch のループ。重ねて鳴る）をこのパネルのスピーカーで鳴らし、`audio-stop` で止める。`audio-spatial` で立体化を外し（有効で光る）、`audio-layout-auto` / `-mono` / `-stereo` でスピーカーの経路を切り替える。`audio-status` に再生中の数と立体化の手段（E2E は `testkit/tests/audio.rs`） |
| VR | `skeleton-test` | 隣にスケルタル入力を試す画面（`SkeletonTest`）を開く。判定の確認として手ごとに入力の元（`input-source-*`）・クリック / 掴む / ショートカットの状態と強さ（`hand-click-*` / `hand-grab-*` / `hand-shortcut-*`）・ダッシュボードの開閉の回数（`double-press-count`。二回押しと手の長押し）を出し、`hand-tuning` でしきい値と揺れの抑えのスライダー（`HandTuningTest`。`tune-*`）を開く。`skeleton-left` / `skeleton-right` に頭から見た手の関節を手のひら中心で描き（骨の線と、関節ごとの向きのギズモ X 赤・Y 緑・Z 青）、親指の先とほかの指の先が `testing::PINCH_DISTANCE`（2cm）より近いと `pinch-{left,right}-{index,middle,ring,little}` が光る。骨格は `PanelControl::hands`（と `hmd`）でフレームループが毎フレーム渡す（E2E は `testkit/tests/skeleton.rs`） |
| VR | `attach-hmd` / `attach-left` / `attach-right` / `attach-tracker` / `detach` | 取り付け・取り外し（トラッカーは見えている最初のもの） |
| VR | `lock` | 掴めないようにする |
| VR | `menu-held` / `grabbed` | メニューボタン・パネルを掴んでいる間光る（表示のみ） |
| GPUI | `toggle` | 切り替え（`on_click` と状態の見た目） |
| GPUI | `hold-me` | 押している間の見た目（`.active()`） |
| GPUI | `drag-chip` → `drop-zone` | ドラッグ＆ドロップ（`on_drag` / `drag_over` / `on_drop`） |
| GPUI | `open-menu` → `menu-a` / `menu-b` | ポップアップ（`deferred(anchored())`、`on_mouse_down_out` で閉じる） |
| GPUI | `text-field` | フォーカス（`track_focus` / `.focus()`）と文字入力（`on_key_down`。キーボードは無いので testkit の `NoVrKit::type_keys` で GPUI に直接打って確かめる） |
| GPUI | `scroll-list` | スクロール（`on_scroll_wheel` / `overflow_y_scroll` + `ScrollHandle`） |

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

## 規約

- 語彙は OpenXR に合わせる: プロファイル `"valve/index_controller"`、入力 `"trigger/value"`。OpenVR バックエンドが翻訳する。
- 座標: トラッキング空間はメートル・前方 -Z。パネル表面はローカル +Z。パネルのピクセル座標は左上原点。
- 新しいコントローラ対応 = `crates/subsystems/input` の `PROFILES` に 1 行追加。上位層でプロファイル名を見ない。
- 新しいランタイム = `crates/subsystems/vr` に `VrBackend` 実装を追加し `connect` に登録する。
- `CODE_OF_CONDUCT.md` に従う: コメントは日本語、Conventional Commits、CalVer（現在 `2026.0.1`、`[workspace.package]` で一括管理）、単体テスト必須、層をまたぐものは結合テスト。
- `// ponytail:` コメントは意図的な簡略化。限界と次の一手が書いてある。
- **ナビゲーションバー（厳守）**: 新しいパネルを開くときは、ナビゲーションバーを付けるか（既定）`PanelSpec::without_nav_bar()` で外すかを `crates/design/GUIDELINES.md` の「ナビゲーションバー」に従って必ず判断する。付けるなら `.titled("…")` でタイトルを渡し、パネルの中に閉じるボタン・同じ見出しを埋め込まない。testkit のキットも本番と同じく持ち主の直後（PanelId = 持ち主 + 1）にバーを開く（キットが直接作る最初のパネルには付かない）。

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

## パネルの LOD

- `config.toml` の `lod_quality`: 0 = Off（常に LOD0）、1 = Low、2 = Mid（既定）、3 = High。`Kernel::panel_lod` が HMD とパネル中心の距離から決める。距離はパネルの密度（1m あたりの論理ピクセル）で換算し、基準（1000px/m）より粗いパネルは近くにあるものとして扱う（拡大すると LOD が下がり、ぼやけたまま残らない。Issue #51）。
- LOD0〜3 は縦横 1 / 1/2 / 1/4 / 1/8。切り替え距離は Low が 1 / 2 / 4 m、Mid が 2 / 4 / 8 m、High が 4 / 8 / 16 m。
- フレームレートも LOD で絞る。LOD0 は毎フレーム、LOD1〜3 は 2 / 4 / 8 フレームに 1 回だけ描き直す（`Kernel::panel_frame_due`。パネルごとにずらす）。間引いたフレームの入力・変化は次に描いてよいフレームで描く。
- GPUI は非表示ウィンドウの DPI と実ピクセルサイズを変える。カーネルの論理解像度と物理サイズは変えない。8 の倍数でないサイズは表示領域に最大 7 論理ピクセルの丸めがある。
- `cargo test -p ui --test panel_lod -- --ignored` で実描画と入力を確認し、`--off` を追加して無効時も確認する（Windows）。
- ストレステストは `NYAW_STRESS_LOD=0` と `2` で同じ負荷を比較できる。

## パネルの表示アニメーション

- VR パネルは中央をアンカーに、サイズ 75%→100%・不透明度 0%→100% で表示し（200ms）、閉じると逆方向に消える（150ms）。
- `animato` で実時間を使って進め、最初の描画が届いてから開始する。途中で閉じても現在の状態から消え、入力は即座に止まる。
- `PanelSpec::without_animation()` で無効化。作成後は `PanelCommand::Animated(false)`、Testing の Animation ボタンでも切り替えられる。レーザー・カーソル・ドラッグ表示は既定で無効。
- フェードは OpenVR の `SetOverlayAlpha`、OpenXR の `XR_KHR_composition_layer_color_scale_bias` で合成し、再生中の画像の再転送を避ける。不透明度 API の無いランタイムだけ、加工前の描画を保持して RGBA を減衰させて転送する。デスクトップの OS ウィンドウは対象外。
- `cargo test -p overlay --test animation_runtime -- --ignored` で起動済み SteamVR でアニメーション画像を検証（HMD 必須）。

## 未実装（土台のみ）

- 実ランタイムでの確認の残り。OpenVR（`openvr_bindings` の C API の関数表。openvr_api は jsoncpp が OpenXR ローダーと衝突するのでリンクせず、`openvr/client.rs` が vrclient を直接読む）は null ドライバで接続・オーバーレイへの送信（Vulkan テクスチャ、毎フレーム更新）まで確認済みで、コントローラ入力は未確認。OpenXR（XR_EXTX_overlay + XR_KHR_vulkan_enable2）は Monado（Windows、simulated）でパネルが映るところまで確認済みで、コントローラ入力は未確認。
- 描画は毎フレーム CPU に読み戻し、変化があれば `vr` が Vulkan のイメージへ上げ直してランタイムへ渡す（`crates/subsystems/vr/src/gpu.rs`。1024x768 で約 1.6ms）。GPUI のテクスチャを読み戻さずに直接共有する経路は未実装。
- Windows での OpenXR ランタイム検出（レジストリ）。
- `gpui` は crates.io 版では `Window::dispatch_event` を外部から呼べないため、Zed の git リビジョンに固定している。`render_to_image` のため `rendering` は本番でも gpui の `test-support` フィーチャーを有効にしている。

## 描画層（Issue #41）

- `rendering` は `overlay` の下、`vr` の上に入り、カーネルが `kernel::rendering` として公開する。UI から描画クレートへ直接依存しない。
- `overlay::Panel` が持つ `rendering::Surface` が描画資源と送信済み状態を保持する。配置・入力判定・アニメーションの目標と進行は overlay が決め、描画先へ送るときの能力差は Surface が吸収する。
- `ui::{capture, Frame, set_render_lod}` は rendering の再エクスポート。`ui::{open_panel_window, panel_window_options}` は UI の表示方針で、OS・描画ホスト固有の作成と読み戻しは `crates/subsystems/rendering` が担う。
- `cargo test -p rendering --all-features` は画素変換、アルファ対応 / 未対応、追従のフォールバック、解放後の更新抑止を検証する。実描画は既存の `gpu_capture` / `panel_lod` で描画層からカーネルまで通す。

## Subsystems（Issue #42）

このプログラムが受け入れる対象でも、OS・ランタイム・コントローラ・描画先によってその形式や能力は違う。カーネル最下部の `kernel::subsystems` がその差を吸収し、上位は共通の状態と操作だけを扱う。

```text
アプリ
├ UI（GPUI）
├ オーバーレイ（パネル管理）
└ カーネル
  ├ 設定・フレーム進行・パネル操作
  └ Subsystems（crates/subsystems）
    ├ os/         Windows / Linux → 共通のパス・設定・OS 操作
    ├ vr/         OpenVR / OpenXR / novr → 共通のランタイム操作
    ├ input/      コントローラのプロファイル → 共通アクション
    ├ rendering/  GPUI・描画転送・能力差のフォールバック
    ├ audio/      OS の音声出力・立体化
    ├ desktop/    画面・ウィンドウの取得と OS タッチ注入
    └ keyboard/   仮想キーボードの配列データ（OS への入力は os/ の os::keyboard）
```

- 各実装クレートは `crates/subsystems/` 配下に置く。共通のライフサイクルトレイトや登録機構は設けない。
- `Subsystems<B>` がバックエンドと機種・手ごとの入力履歴を所有する。`poll()` の中で取得・機種別変換・ヒステリシスを完結し、切断や機種変更では履歴を捨てる。
- 上位へ渡す `subsystems::Frame` は `ControllerState` の配列と姿勢・トラッカー・終了要求を持つ。生入力・機種プロファイル・ランタイムの `emulated` フラグは含めない。`pose()` は追跡可能な姿勢だけを返し、`stabilize_pointer` はポインタ補正の要否を表す。
- オーバーレイは共通アクションからヒット判定・クリック・掴み・スクロールを処理する。機種別の入力名・押下しきい値・入力履歴の管理はしない。パネル操作に必要なインタラクションの状態はオーバーレイに置く。
- ランタイム指定の解釈・novr へのフォールバックは `Kernel::boot`、タイマー精度・GPU 優先度を設定する判断は `Kernel::with_backend` が行う。Subsystems は接続・OS 設定の実行手段を提供する。設定の優先順位は呼び出し側の責務。
- テストの入力注入と描画結果の確認は `kernel.subsystems.backend` を使う（旧 `kernel.vr`）。本番のフレーム取得は必ず `Subsystems::poll` を通す。既存の `kernel::vr` / `kernel::rendering` の型の再エクスポートは維持する。
- OS 差は `os`、描画ホストの差やランタイムの描画能力差は `rendering` に閉じる。新環境への対応で上位へ環境別分岐を増やさない。
- `cargo test -p subsystems -p kernel -p overlay` で機種差・左右独立の押下判定・切断と機種変更・追跡能力・パネル操作への接続を検証する。

### 共通方針の所有者

- ログのファイル出力・世代管理は `logger`、`NYAW_LOG` の取得は `kernel::config`。OS サブシステムは保存先の違いを吸収する。
- パネルを閉じた後、フェード完了で資源を解放する判断は `overlay`。`rendering::Surface` は `release` を実行し、パネルの閉鎖状態を受け取らない。
- 非表示・透過・フォーカスしないというウィンドウの方針は `ui`。Windows の描画先サイズ補正や Linux の headless での描画、LOD の拡大率の変え方（vendor/ の `with_render_scale`）など、実現手段の違いは描画サブシステムが吸収する。
- Subsystems とのやり取りがあるだけでは、その共通機能を Subsystems の一部にしない。同名クレートを上下に作らず、ログ・起動・パネル管理などの所有者へ置く。

## 画面・ウィンドウ配信

- `crates/subsystems/desktop` が取得元の一覧、画像取得、OS タッチ注入を担う。Windows は WGC と Synthetic Pointer、Linux は XDG Desktop Portal の ScreenCast / RemoteDesktop と PipeWire。上位へは `kernel::desktop` 経由で公開する。
- `Sources::refresh` と `Stream::start` はワーカーを起こすだけ。メインスレッドは `poll` で最新画像だけを受け取り、OS の開始・取得・停止・入力注入を待たない。入力キューが詰まったら接点を残さずセッションを止める。
- `PanelCommand::Stream(Some(source))` で既存パネルを配信先にし、`None` で通常の UI に戻す。`PanelStatus::desktop` が開始中・配信中・終了・エラーを持つ。画像はカーネルから直接 overlay へ渡し、UI の描画で上書きしない。横幅を保って配信元の縦横比に追従する（ダッシュボードにはめている間は縦を保つ）。
- 配信先のポインタは GPUI のマウス変換を通さず、左右を ID 0 / 1 のタッチ接点としてサブシステムへ送る。パネル外でも解放まで捕捉し、手の追跡喪失・掴み開始でキャンセルする。閉鎖・エラー・再接続では OS セッションを解放する。
- Testing の `desktop-test` が一覧更新・配信開始・停止と状態表示を持つ。配信パネルは独立して掴んで移動でき、ナビゲーションバー（タイトルは取得元の名前）で閉じると配信も止まる。バーの左端の `nav-keyboard`（`PanelSpec::with_os_keyboard`）で OS キーボード（OS の前面のアプリへ送る仮想キーボード）を出し入れし、出ている間は強調色になる。キーボードはバーの下に出る。統計の HUD と仮想キーボード（自前の X）はバーを付けない。
- 下位の確認は `cargo test -p desktop -p kernel -p overlay -p subsystems --no-default-features --lib`。UI の単体テストは `cargo test -p testing -p ui -p i18n --lib`。`desktop/test-support` のモックは最新画像とタッチ配送、セッション停止を検査できる。
- 実デスクトップの E2E は `crates/subsystems/desktop/tests/{windows_capture,linux_capture}.rs`、UI を通すシナリオは `crates/testkit/tests/desktop_stream.rs`。すべて `#[ignore]`。実行には明示的な指定と、ネイティブのテストでは操作できるデスクトップが必要。`cargo check --tests` / Clippy は OS を操作せず型を検査できる。
- Linux のビルドには `libpipewire-0.3-dev` と `libclang-dev` が必要。Portal がタッチ操作を許可しなければエラーを返す。Windows のウィンドウ操作はデスクトップで対象が見えている場合に限り、別ウィンドウへの誤入力を防ぐ。
## 仮想キーボード

キーボードは役割ごとに 3 か所に分かれる。

- `crates/subsystems/keyboard`: 配列データ（xkeyboard-config）と、押したキーから文字・修飾キーの状態を求める（`Stroke`）。OS には触らない。
- `crates/subsystems/os` の `os::keyboard`: OS の前面のアプリの配列・IME の監視（`Watcher`。別スレッド）と、キー・文字の注入。カーネルが持ち（`Kernel::os_keyboard`。最初に使ったときに監視を始める）、`PanelCommand::TypeToOs` で送る。
- `crates/ui` の `ui::keyboard`: キーボードのパネル（`KeyboardView`）と、入力をどのパネルへ届けるかの振り分け（`InputRouting`）。GPUI の入力欄は `focus_input`、OS を操作するパネル（配信・`PanelSpec::with_os_keyboard`）は `toggle_os_keyboard` で出す。OS の状態はフレームループが `PanelControl::os_keyboard` に書き写し、`system_keyboard` で読む。

## 音声（パネルのスピーカー）

- どのパネルにもスピーカー（音源）があり、パネルの中で鳴らす音はそこから聞こえる。`PanelCommand::Play(audio::Sound, looping)` で鳴らし（読み込みは別スレッド。同じパネルでも別のパネルでも、いくつでも重ねて鳴る）、`StopAudio` でそのパネルの音を止める。パネルを閉じると止まる。`PanelStatus::audio` に再生中の数・設定・立体化の手段。
- `PanelCommand::PlayAt { sound, at, pitch }` はパネルの中の点（ピクセル）から一度だけ鳴らす。その点に置いた 1 点の音源（左右を混ぜる）で、パネルごとに 8 個を順に使い回し、鳴らした時の置き場所に留まる。`pitch` は速さの倍率（`audio::Clip::pitched`）。
- 仮想キーボード（`ui::keyboard::KeyboardView`）はキーを押すと、押した点から `PlayAt` でキーの音（`crates/ui/assets/keyboard/*.wav`。Enter・Backspace・Space は専用、ほかは tap）を鳴らす。音の高さは押すたびに 1 ± 0.1 で揺らす。
- 立体化は既定で有効（オプトアウト）。`PanelCommand::Spatial(false)` で外すと、置き場所・頭の向きに関係なくそのまま左右に鳴る。
- スピーカーはモノラル（パネルの中央の 1 点）とステレオ（パネルの左右の端の 2 点。間隔はパネルの幅）。既定の `Layout::Auto` は鳴らす音に合わせ、1ch と左右がまったく同じ 2ch はモノラル、それ以外はステレオ。`PanelCommand::SpeakerLayout(Layout::Mono | Layout::Stereo)` で固定できる（モノラルは左右を混ぜ、ステレオはモノラルの音を両端から鳴らす）。
- 聞き手は HMD。カーネルが毎 tick、鳴っているスピーカーだけをパネルの姿勢に合わせて動かす（鳴っていなければ何も送らない）。距離は 1m までは減衰せず、その先は逆距離で減る（Steam Audio の既定の逆距離モデル）。
- 立体化は Steam Audio の HRTF（バイノーラル）。ライブラリ（Windows `phonon.dll`、Linux `libphonon.so`。Steam Audio 4.8 の SDK の `lib/windows-x64` / `lib/linux-x64`）は実行時に読む。`NYAW_PHONON` でファイルを指すか、実行ファイルの隣（Windows）・ライブラリの探索パスに置く。無ければ左右の等パワーのパンニングに落ちる（`Spatializer::Panning`）。ビルドに clang や Steam Audio は要らない（`audio/src/phonon.rs` が `phonon.h` から使う型と関数だけを写している）。
- ミックスは 256 フレームごと。HRTF の状態は最初に 64 個（32 音 × L / R）作っておき、コールバックの中では作らない。超えた音はパンニングで鳴る。出力のレートへは線形補間で合わせる。
- テスト用のカーネル（`Kernel::with_backend`）はデバイスを開かない `audio::Engine::offline`（パンニング）で、テストが `kernel.audio.render` で回して左右に出た音を測る。本番の `Kernel::boot` は `Engine::open`（デバイスを開く・Steam Audio を読むのは別スレッド）。
- `cargo test -p audio -p kernel --lib`、UI を通すシナリオは `cargo test -p testkit --test audio`。実際の Steam Audio とデバイスは `cargo test -p audio -- --ignored`（音が出る。`NYAW_PHONON` で phonon を指す）。
- Linux のビルドには `libasound2-dev`（ALSA。cpal が使う）が必要。
