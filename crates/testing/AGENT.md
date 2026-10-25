# crates/testing

層: 5 画面

UI 層の上に載る画面。GPUI のビューで、ランタイムを意識しない

## 主な公開 API

`Testing`（テスト用の画面）, `TestingAnim`（毎フレーム変わる）, `TestingLod`（LOD を数字と色で表示）, `TestingScrollbar`（縦・横・縦横のスクロールバー）, `StressTest`（上限までパネルを出す・閉じる・ちりばめる・並べる）, `TooltipTest`（ツールチップ）, `SmoothingTest`（レーザーの手ブレ抑えの強さと線の長さをスライダーで調整）

## Testing（テスト用の画面）

一連の機能を1画面で試せる画面。新しい機能を足したら Testing にも操作を足し、E2E はその操作で書く。有効な状態は `BUTTON_ACTIVE_BG` で光る。

| 区分 | 要素（`debug_selector`） | 試せること |
|---|---|---|
| VR | `click-me` | クリック・ホバー（全機種・左右の手） |
| VR | `spawn-panel` | 隣に Testing パネルを追加し、その画面を独立して操作する |
| VR | `locale-test` | 隣にローカライズを確かめる画面（`LocaleTest`）を開く。`lang-en` / `lang-ja` で言語を切り替え（設定ファイルに残る）、`locale-missing` に訳の抜け、`locale-samples` に長い文・差し込み・ボタン、`locale-table` に全キーを言語ごとに並べる。Testing の `toggle-language` でも次の言語へ切り替わる |
| VR | `tooltip-test` | 隣にツールチップを試す画面（`TooltipTest`）を開く。`tip-short` / `tip-long`（隣り合い、移ると文字と幅が変わる）、`no-tip`（説明なし）、`tip-overflow`（箱に収まらない説明。止まる → 一周流れる、を繰り返す）、アイコンだけのボタン `tip-icon-*`（説明は必須）、`tip-far`（離れていて猶予が切れる）。`tip-status` に手ごとの文字と向き |
| VR | `pointer-test` | 隣にポインタの方式を試す画面（`PointerTest`）を開く。`pointer-mode` でマウス / タッチのエミュレートを切り替え（タッチで光る）、`drag-chip` → `drop-zone`（マウスのドラッグ）・`swipe-list`（タッチのスワイプ）・`tap-me`（どちらでも押せる） |
| VR | `dialog-test` | 隣に親子関係を試す画面（`DialogTest`）を開く。`open-dialog`（外側を押すと閉じる既定のダイアログ）と `open-modal-dialog`（`without_dismiss`。答えるまで残る）でダイアログ（`Dialog`。`PanelSpec::child(.., carry = true, ..)`）を子として出す。開いている間は親が暗くなって押せず（掴んで動かすと子が付いてくる）、ダイアログを掴むと親も付いてくる（独立しない）。`dialog-ok` / `dialog-cancel` で答えて閉じる |
| VR | `settings-test` | 隣に設定画面（`Settings`）を開く。GNOME の設定のようなツートーン: 左のサイドバー（地は `sidebar`）のカテゴリ（`settings-general` / `settings-appearance` / `settings-pointer` / `settings-about`。選んでいるものだけ文字が濃く、左端にアクセント色の印が付く）を押すと、右（地は `background`）の中身が切り替わる。中身の行（`setting-lang-*` / `setting-theme-*` / `setting-pointer-*`）とスライダー（`setting-smoothing` / `setting-release` / `setting-laser-length`）はすぐ全体の設定（`Setting`）に反映する。閉じるのはナビゲーションバー |
| VR | `nyapp-test` | 隣にニャプリの一覧と詳細（`NyappTest`）を開く。左にシステムのものと入っているもの（`nyapp-row-<id>`。`nyapp-rescan` で読み直す）、右に選んだものの ID・種類・版・ABI・権限・更新情報・アイコン（パッケージのもの / 代わりのもの）・パッケージ・起動中のパネル（`nyapp-detail`） |
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
