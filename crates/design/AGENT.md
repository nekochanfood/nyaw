# crates/design

層: 4 UI（横断部品）

UI の見た目の責務。Radix Colors のエイリアスに倣う 3 層。Constants（`scale::{light,dark}::SLATE_1..12` / `BLUE_*` / `RED_*`、`scale::{WHITE,BLACK}`、`radius` / `spacing` / `font_size` / `letter_spacing` / `duration_ms`。段ごとに用途が決まった 12 段の生の値）→ Utilities（`Theme`。`accent` → `blue` のスケールの別名と段の用途名 `accent_solid` など、テーマで指す先が変わる `background`（パネルの地）/ `card` / `control`（ボタンの既定）/ `groove`（スライダーの空き）/ `track` / `on_accent` など。`Mode::{Dark, Light}` と `Theme::of`。部品は役割だけを見る）→ Components（`button` / `switch` / `slider` / `panel` + `card`（カード複数）/ `card_panel`（カード 1 枚）/ `navigation`（画面の積み重ね。右から入る push 遷移と、押したまま右から左へ動かすスワイプバック。タッチ・マウスとも。中の要素が止めた入力は奪わず、縦の一覧は `restrict_scroll_to_axis()` を付ける）/ `dialog`（子パネルの中身: アイコン・タイトル・概要・ボタン）/ `nav_bar`（パネルの下に付くナビゲーションバー: 中央にタイトル、右端に閉じるボタン `nav-close`。持ち主のパネル（かバー）を指していない間は丸い線に縮み、指すと広がる）/ `list` + `list_item`（カード色の行を区切り線で並べる一覧）/ `menu_item`（`>` の無い行。メニュー用）/ `tile`（地に直接並べる押せるカード。グリッド用。中身は呼び出し側が足す。`Button` は `ParentElement` なので中身を足せる）/ `select` + `select_option` + `switch_item`（オン / オフの行: `menu_item` の右に `switch`（`{id}-switch`）。押したときの処理は呼び出し側）, `select_popover`（複数から 1 つ選ぶ行: 右に今の値と `caret-up-down`。押すと行の右下に右端を揃えたポップオーバーに選択肢が並び、選んでいる行にチェック。外を押すと背面の透明な板が受けて閉じる）/ `dropdown` + `dropdown_popover`（行の中ではなく単独で置く、複数から 1 つ選ぶボタン: 今の値と `caret-up-down`。選択肢は `select_popover` と同じカードを、ボタンの下か上（`up`）に左右中央を揃えて出す）/ `disabled_button`（押せないボタン: 今その状態であること（「設定中」など）を、枠だけのカプセルとチェックの印で示す。色が変わらず、波紋も音も出ない）/ `group`（小さな見出しの下に中身。設定の項目をまとめる）/ `carousel` + `arc`（下の「カルーセル」）/ `sidebar` + `sidebar_item`（サイドバーの行。地は `sidebar`（GNOME 風のツートーン）で、背景が無く、選んでいる行だけ文字が `neutral-text` になる。ほかは `neutral-text-low`。左端のアクセント色の印は行ごとに `sidebar` が描き、選ぶ行が変わると前の印は中央へ縮んで消え、同時に新しい印が中央から伸びて出る（印は行の丸い背景の内側に `SIDEBAR_INDICATOR_INSET` 空けて置く。消えるのは `Motion::Hop`（ease-out）、出るのは `Motion::HopIn`（ease-in-out）。どちらも 300ms。上下の伸び縮みと 1:1 の点の拡大縮小をつなぎ、幅より低く潰れない））/ `surface`）。値は `tokens.json`（色は Radix Colors を OKLCH にしたもの）が唯一の情報源で、build.rs が `palette` クレートで sRGB にしてコードを作り、役割の組が WCAG のコントラスト比を満たさなければビルドを落とす

## 主な公開 API

`icon(&icons::HEART).weight(Weight::Fill).size(..).color(..).rotate(..)`（Phosphor を path で描く。色は `Background` なのでグラデーションでアイコンの形に抜ける。6 ウェイト全対応。**ソースで使われているアイコン・ウェイトだけ**を build.rs が埋め込む。データは `icons/*.tsv`、更新は `icons/vendor.mjs`）, `Theme::{DARK, LIGHT, of, current, set_mode, fading, map, zip, grayscale}`（`grayscale` は白黒にしたテーマ。止まったニャプリの画面。`set_mode` でテーマを切り替えると、`Motion` があれば `current` が全役割を前の色から `duration_ms::SLOW` かけて OKLab で補間する。その間 `fading` が真で、`ui` のフレームループは全パネルを描き直す）, `Mode`, `logo(Logo::{Full, Mini}).height(..)`（nyaw のロゴ。今のテーマに合う SVG を `assets/logos/` から埋め込み、裏のスレッドで描く。`.mode(..)` で固定。exe のアイコンは別で `assets/icons/*.ico` をルートの `build.rs` が埋め込み、ウィンドウ側は `vendor/gpui_windows` の `load_icon` がアプリのテーマを見て ID 1（ライト）/ 2（ダーク）を選ぶ。Explorer のファイルアイコンはテーマで切り替わらない）, `logo(Logo::{Full, Mini}).height(..)`（nyaw のロゴ。今のテーマに合う SVG を `assets/logos/` から埋め込み、裏のスレッドで描く。`.mode(..)` で固定。exe のアイコンは別で `assets/icons/*.ico` をルートの `build.rs` が埋め込み、ウィンドウ側は `vendor/gpui_windows` の `load_icon` がアプリのテーマを見て ID 1（ライト）/ 2（ダーク）を選ぶ。Explorer のファイルアイコンはテーマで切り替わらない）, `components::{button, icon_button, round_button, switch, slider, panel, card, card_panel, dialog, PANEL_MARGIN, surface, tooltip, Tooltips, hover_tooltip, Variant}`（`Variant::{Default, Primary, Danger, OnBackground, Ghost}`。既定は無彩色。`Ghost` は背景の無いボタン）, `content_button`（中身を足す横長のボタン）, `progress_ring`（線の円の進み具合。上から時計回り）, `markdown(theme, id, source, window, cx)`（Markdown を読める形で縦に並べる。リリースノート用。GitHub の書き方（表・打ち消し線・チェックリスト・裸の URL のリンク）まで。解析は pulldown-cmark、HTML は描かない。リンクは押すとブラウザで開く。ブロックの `debug_selector` は `{id}-{添字}`。`id` ごとのビュー（`use_keyed_state`）で解析した結果と描いた絵を覚え、`source`・テーマ・置き場所が変わるまで gpui のビューのキャッシュ（`cached`）で使い回す。キャッシュは高さを先に決める必要があるので、最初は使い回さずに描いて中身の高さを測る。`refresh` はキャッシュを捨てるので、毎フレーム描き直すパネルでもフレームループは根（`PanelRoot`）だけ汚す。テストは `components::markdown::tests`（解析）と `tests/markdown.rs`（描いた並び・折り返し・リンク・使い回し））, `scale::*`, `drive::{now, advance, keep_drawing}`（動きの駆動: 時計と再描画の要求。部品は `Instant::now()` を直接読まない）, `cue::{Cue, emit, take, hover, begin_hover}`（効果音の要求。部品は種類 `Cue` を置くだけで音は知らない。同じ種類は 50ms 以内なら捨てる。音の割り当ては `ui::cue`）, `corner::{SUPERELLIPSE, ENABLED, install}`（UI 全域の角丸をスーパーエリプス k=1.2 にする。`ENABLED` で全域を切り替え、`ui::run` が `install` を呼ぶので呼び出し側は `.rounded(..)` だけでよい。個別に変えるのは `.corner_superellipse(k)`、オプトアウトは実装側で `.circular_corners()`（円の角丸。円であるべき丸ボタン・カプセルなど）。実装は vendor/gpui のシェーダー）, `components::{scrollbars, elastic, claim_scroll, Scroll}`（スクロールバーと端の引っ張り。`&ScrollHandle` のほか、見えている行だけを作る gpui の `list` の `&ListState` も渡せる。`list` は `elastic` で一覧ごと包む）, `catalog::Catalog`（デバッグ用カタログ。色の段・役割・定数・文字・アイコン・部品・動きを 1 画面で確認する。節を gpui の `list` に並べ、見えている節だけを作る。`cargo run -p design --example catalog`）。見た目の方針は `crates/design/GUIDELINES.md

## パーソナルカラー

ユーザーが決める、アクセントの**塗り**の色（`Accent`。OKLCH の明るさ・鮮やかさ・色相）。広くは効かせない: 差し替えるのは `accent-solid`（プライマリボタン・スライダーの埋まった側・オンのスイッチ・サイドバーの印など）と、そのホバー・押下、上に載る `on-accent` の 4 つだけ。スケールのほかの段（色付きの背景・枠・文字・`focus-ring`）は tokens.json のまま。

- **自由なのは塗りの色**。選んだ色は、どのテーマでもそのまま塗りになる（画面に出せない鮮やかさだけ落とす。`Accent::color`）。カードに紛れる色を選ぶのもユーザーの自由で、こちらでは補正しない。
- **従うのは周りの色**（`fill(mode, accent)`）:
  - 上に載る色（`on-accent`）は、塗り・ホバー・押下のどれの上でも 3:1 で読める方。白で読めれば白（既定と同じ）、読めなければ黒。
  - ホバー・押下は、既定の塗りと同じコントラスト比だけ暗くする（黒に近い塗りでは、同じ比だけ明るくする）。黒が載るときは黒が読める明るさより暗くせず、押下がスクロールバーのつまみ（`track`）と同じ明るさになるなら避ける。
- `Theme::with(mode, accent)` がその 4 つを差し替えたテーマ。今の色は GPUI のグローバル `Accent`（`Theme::set_accent`。テーマと同じく今の色から移り変わる）で、`Theme::current(cx)` が反映する。既定（`Accent::DEFAULT` = tokens.json の `accent-solid`）なら tokens.json のテーマそのもの。
- 選ぶ画面に並ぶのは**履歴**（`Accent::history(今の色, 作った色)`）。自分で作った色（新しい順）の後ろを用意した色 `Accent::PRESETS`（名前は i18n の `color.<名前>`）で埋めたもので、作っていなければ用意した色そのもの。用意した色は「履歴の最初の中身」でしかなく、作るたびに後ろから落ちる（覚えるのは `kernel` の `color_history`）。
- 選ぶ部品は `components::accent_row`（履歴の見本 `swatch` を左右中央に並べ、右端に自分で色を作るプラス `accent-custom`）。色を作るのは別の画面（ダイアログ。`ui::prefs::ColorDialog`）で、`components::accent_sliders`（色相・鮮やかさ・明るさのスライダー。上の帯は動かすとどう変わるか）と見本を出す。
- 結果を見せるのは `components::theme_preview(theme)`（文字の無い絵: 地の上に重なった 2 枚の窓。手前の窓に、文字に見立てた線と塗りの色のボタン）。名前の無い色は `Accent::hex`（`#RRGGBB`）を説明に出す。
- テスト: `accent::tests`（色相・鮮やかさ・明るさを一巡りして、上の色が読めること・状態が見分けられること）。実 GPU の絵は `cargo test -p ui --test gpu -- --ignored look`（`target/tmp/look-*.bmp`）。

## カルーセル

横に並ぶ項目を左右に滑らせて 1 つ選ぶ（端はつながって回る）。責務を 2 つに分ける。

- **`components::carousel(id, len, selected, shape, render, on_select)`**（`components/carousel.rs`）: どんな並べ方でも通用する部分。項目のループ（`offset`）、押したまま左右に動かす操作（タッチの指のスライドとマウスの両方）、離したときに収まる項目（近い項目。払っていればその先。`settle`）、選ぶ項目が外から変わったときに近い向きへ回る動き、中央でない項目を押すと中央に来ること。ここで始まった横の動きは外へ渡さない（`navigation` のスワイプバックと取り合わない。縦の動きは渡す）。選ぶ項目は呼び出し側が持つ。
- **並べ方 `Shape`**: 中央から `d` 個ずれた項目（小数は動いている途中）の置き場所 `Place`（中心・一辺・不透明度）と、項目 1 つ分の指の動き `step`・高さ・描く範囲 `reach`。形に特化した計算はここにだけ書く。
  - **`components::arc(diameter)`**（`components/carousel/arc.rs`）: 大きな円の弧に沿う並べ方。中央（弧の頂点）の項目の一辺が `diameter`。隣は弧に沿って下がり、薄くなり、弧の向きに傾く（大きさはどこでも同じ）（`Place::angle`。描くときに `Window::with_rotation` で回す）。その先は消えて見えない。wrist の文字盤を選ぶ画面（`ui::prefs::watch_faces`）が使う。
- 別の形（一直線・カバーフローなど）が要るときは、`Shape::new` を返す関数を足す（`carousel` は変えない）。
- テスト: `carousel::tests` / `carousel::arc::tests`（計算）、`tests/carousel.rs`（操作）。

## 文字の大きさと太字化

アクセシビリティの設定（`kernel` の `text_scale` / `bold_text`）を、すべての文字に効かせる仕組み。**掛けるのは下の層**（vendor/gpui のテキストシステム）で、画面や部品のコードは今までどおり等倍の大きさ・太さ（`font_size::MD`、`FontWeight::NORMAL` など）を書くだけでよい。これから書くコードも、何も考えなくても効く。

- **入口**: `font::set_text(scale, bold, cx)`（`ui::sync_settings` が全体の設定から書く。変われば全ウィンドウを描き直す）。
- **大きさ**: GPUI が文字を組む所（`WindowTextSystem::shape_line` / `shape_text` / `layout_line*`、折り返しの `TextSystem::line_wrapper`）が、渡された大きさに倍率を掛けて組む。要素として置いた文字も、`shape_line` を直接呼んで自前で組む・測る文字も、同じ所を通る。行の高さ（`TextStyle::line_height_in_pixels` と文字の要素）も同じ倍率で伸びる（相対でも `px` でも）。`window.text_style()` が返すのはコードに書いた値のまま。
- **太さ**: 書体を決める所（`TextSystem::font_id`）が、`BOLD` より細い指定を `BOLD` にする。すでに太い字はそのまま（より太い書体は積んでいない。太字どうしの差を付けたくなったら、同じ配布物の ExtraBold を積んで対応を足す）。等幅など太さが 1 つの書体は OS が太らせる。
- **アイコン・絵文字**: `icon(..)` と `emoji(..)` は文字と並ぶものなので、自分で同じ倍率に付いていく（`gpui::text_scale()`）。
- **外す（オプトアウト）**: `.text_exact()` を付けた部分木は、大きさも太さもコードに書いたとおりに描く（中の `icon` も等倍）。絵の一部になっている文字だけに使う: wrist の文字盤、仮想キーボードのキーの文字、アプリの頭文字アイコン。
- **文字に合わせた寸法**: 文字そのものには何も要らないが、「1 行の文字を入れる高さ」のように文字から作った箱は `font::scaled(長さ)` で倍率に合わせる（ダイアログの行、ツールチップの行、ダッシュボードのタイル）。高さを決めず中身に任せた箱（`h` を書かない）は何もしなくてよい。部品（ボタン・行・ナビゲーションバー）の高さは、いちばん大きい倍率の文字が収まるように決めてあるので変えない。
- **文字の幅を測る**: `font::width(text, size, weight, window)`（既定の書体で 1 行に組んだ幅。倍率・太字化が掛かった、画面に出るとおりの幅）。`TextRun` を組んで `shape_line` を呼ぶ定型を画面や部品に書かない。
- **状態の置き場所**: スレッドごと（GPUI は 1 つのスレッドで組んで描く。テストは 1 本ずつ別のスレッドなので互いに影響しない）。
- **設定の部品**: 行の中にスライダーを置く `components::slider_item`（値の文字は名前の側、スライダーは右端。文字の大きさが変わっても動かしている最中のスライダーが動かない）、離したときに決める `Slider::on_release`。
- テスト: `tests/text.rs`（要素の文字・自前で組んだ文字・アイコン・絵文字に効き、`.text_exact()` だけ従わない。倍率と太字化はどの組でも独立に効く）、`tests/dialog.rs`（高さが倍率に付いていく）。実 GPU の絵は `cargo test -p ui --test gpu -- --ignored a11y`（`target/tmp/look-a11y-*.bmp`）。
