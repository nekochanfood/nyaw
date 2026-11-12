# crates/wrist

層: 5 画面

常駐パネル。利き手（`Settings::dominant_hand`。意味は `../kernel/AGENT.md`）の反対の手首に腕時計のように取り付き（掴めない。設定が変わると `PanelCommand::Attach` で自分から反対の手首へ付け替える）、丸い文字盤に日付・時刻（見た目は設定の `wrist_design`。下の「文字盤とデザイン」）。常駐なので、X ボタンの二回連続押し（`PanelStatus::double_press` の回数が増える）でダッシュボードを開く（2 回目からは隠す / 出し直す。閉じない）。ダッシュボードから開くデバッグ画面・設定画面は main が渡す（`wrist::spec(dominant, debug, settings)`。`dominant` は main が `Settings::dominant_hand` をそのまま渡し、どちらの手首に付くかは wrist が決める。今は Testing とその設定画面。画面どうしは直接依存しない）。ロックダウン（`Settings::lockdown`。取り手はカーネルの `PanelCommand::Lockdown`）に入った瞬間に、手首自身と出ているダッシュボードを隠し（解けると手首は出し直す）（解けても出し直さない）、中は二回押しを受け取っても何もしない（ダッシュボードを開閉しない）

## 主な公開 API

`wrist::spec(dominant, debug, settings)`, `Wrist`, `WATCH_OFFSET`, `wrist::preview(design, d, cx)`（選ぶ画面に出す文字盤の見本。画面どうしは直接依存しないので、`Wrist::new` が UI 層のグローバル `ui::prefs::FacePreview` に置き、選ぶ画面（`ui::prefs::watch_faces`）がそこから引く）

## 文字盤とデザイン

文字盤には種類（`kernel::WristFace`: スタンダード / デジタル / アナログ）があり、種類ごとにその種類に合う題材のデザイン（`kernel::WristDesign`）を持つ。設定は `wrist_design`（`Setting::WristDesign`）1 つで、種類はデザインから決まる（`WristDesign::face`）。

| 種類 | デザイン |
|---|---|
| スタンダード（中央に日付と大きな時刻。縁で秒を見せる） | `dots` ドット（既定。点の目盛りと秒の点） / `ring` リング（秒で満ちる輪と、塗りの色の日付の札） / `gauge` ゲージ（60 の目盛りが秒まで灯る） |
| デジタル（数字で秒まで） | `lcd` 液晶（7 セグメント。消えたセグメントも薄く見える） / `flip` フリップ時計（時と分の札） / `terminal` ターミナル（等幅の文字と点滅するカーソル） |
| アナログ（針） | `minimal` ミニマル / `dress` ドレス（セリフのローマ数字） / `railway` 鉄道時計（太い目盛りと、先に円の付いた秒針） |

- 色はテーマの役割だけを使う（生の色は書かない）。地は `card`、文字と針は `neutral-text` / `neutral-text-low`、秒を示すもの（点・輪・秒針・カーソル）はパーソナルカラーの塗り `accent-solid`、塗りの上の文字は `on-accent`。なので、テーマとパーソナルカラーを替えると文字盤の色も替わる。
- 寸法は基準の直径（`face::BASE` = 400px、本物の大きさ）で書き、`Face::px` / `Face::size` が実際の大きさに縮める。本物も見本（`preview`）も同じ関数で描く。
- 飾りの書体は `design::font::{serif, segment, mono}`（OFL のものだけ。`../design/src/font.rs`）。

### デザインを足す

1. `crates/kernel/src/config.rs` の `wrist_designs!` の表で、種類の行に `名前 = "キー"` を足す（設定ファイルの値・選ぶ画面の一覧・`WristDesign::face` はここから決まる）。
2. `crates/wrist/src/face/<種類>.rs` に `pub fn キー(f: &Face) -> Div` を書く。材料は `Face`（`frame` / `date` / `time` / `clock` / `hands` / `label` / `paint` と `Pen`）だけを使う。
3. `crates/wrist/src/face.rs` の `dial` の `match` に 1 行足す（足し忘れるとコンパイルが通らない）。
4. `crates/i18n/locales/*.toml` の `[wrist.design]` に名前を足す（足し忘れると `every_face_and_design_has_a_name` が落ちる）。

種類を足すときは、表に行を足し、`face/` にファイルを足す（`[wrist.face]` にも名前）。設定画面・セットアップの選択肢は表から並ぶので変えない。見た目は `cargo test -p ui --test gpu -- --ignored look` で `target/tmp/look-faces-*.bmp` に書き出して確かめる。

文字盤は絵として描くので、アクセシビリティの「文字の大きさ」「太字」には従わない（`Face::frame` の `.text_exact()`。数字も直径に合わせて決めてある）。大きく見たいときは UI スケールで文字盤ごと大きくなる。
