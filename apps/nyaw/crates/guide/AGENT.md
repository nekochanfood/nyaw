# crates/guide

層: 4 UI（横断部品）

コントローラの操作ガイド。コントローラのレンダリング画像を表示し、操作できる部分（スティック・グリップ・ボタン）を円で囲み、テールで見出しと説明文に結ぶ。どの画面からも使える部品で、画面そのものではない（検証画面は [`testing`](../testing/AGENT.md) の `GuideTest`）。

## 主な公開 API

```rust
controller(model, hand)                 // コントローラの画像と説明（ControllerGuide）
    .size(px(400.))                     // 画像の一辺。説明の列は左右に GUTTER（一辺の 0.5 倍）ずつ足す
    .callouts(vec![Callout { .. }])     // 囲む部品と、テールの先の見出し・説明文
    .focus(Some(i));                    // i 番目を強調し、ほかを薄くする
bindings(cx, controller, hand);         // 割り当てのある操作ごとの Callout（札の ID は guide-<操作>）
sheet(model, hand); parts(model, hand, "grip"); // 画像のボタンの表（座標・半径・見えるか）
control_name / control_detail / part_name;      // 表示用の名前（i18n の guide.*）
layout(model, hand, size, &callouts);   // 円・札・テールの配置（描画と同じ計算。テスト用）
zoom(model, hand)                       // 寄って映す（Zoom）。セットアップの完了画面が使う
    .size(w, h).parts(vec!["x"])        // 枠の大きさと、ダブルタップの波紋で示す部品
    .fade(px(160.))                     // 枠の上端を地の色へ溶かす（上に重なる文字と干渉させない）
    .enter(Some(at));                   // この時刻から下から上がりつつ現れる
face(model, hand); framing(model, hand, w, h); double_tap(t); entrance(t); // 寄り方と動き（テスト用）
Model, Controller, Control;             // 入力層（input）の再エクスポート
```

機種の判定（ランタイムが報告したレンダーモデル名 → 割り当て → Quest Touch Plus の順）と、操作ごとの部品（`Controller::parts`）は入力層が持つ（[`input`](../subsystems/input/AGENT.md)）。この crate は機種名を判定しない。

## 画像と表

- **正本**: リポジトリルートの `assets/renders/<left|right>/<レンダーモデル名>.png`（2048 × 2048、背景透過。寄って映しても粗くならない大きさ）と `assets/renders/buttons/<left|right>/<名前>.json`（ボタンの中心・画像上の半径 `radius`・正面から見えるか。丸いボタンは写った輪郭（上面と側面。スティックは頭）を囲む円なので、印の円がボタンにぴったり重なる。上面の中心が隠れた丸いボタン（PICO 4 の system）は見えない部品として扱う）。どちらも `assets/renders/tools` が Blender と SteamVR の部品から作る。手で座標を打たない。
- **埋め込み**: 全 22 枚と表を `include_bytes!` / `include_str!` で埋め込む（約 45MB）。表の座標の大きさ（`coordinates.width`）は `IMAGE_PX` と一致しなければならない（読むときに確かめる）。表は初めて使うときに 1 度だけ読み、PNG のデコードは GPUI のアセット（バックグラウンド）が行う。
- 機種を足すときは、`assets/renders` に画像と表を作り、入力層の `MODELS` と、この crate の `ASSETS` に 1 行ずつ足す（`every_model_has_a_render` が漏れを検出する）。

## 描き方

- **円**: 見えるボタンだけを、ボタンの縁から `RING_PAD`（9px）離した円で囲む。線（3px のアクセント色、影なし）の内側にも隙間が残り、ボタンの形と刻印が見える。UI の角丸はスーパーエリプスなので、円は `circular_corners` で真円に戻す。小さく写るボタンも最小の半径（`RING_MIN`）を保ち、握り全体のような大きな部品は `RING_MAX` で打ち切る。
- **見えない部品**（トリガーなど裏側）: 円を描かず、破線のテールだけを部品の位置へ伸ばす。
- **札とテール**: 札は部品が画像の左右どちら側にあるかで左右の列に置き、部品の高さに合わせる。重なる分だけ下へ（下端を越えたら上へ）ずらす。片側に入りきらなければ中央寄りの部品から反対側へ回し、それでも入らなければ札を低くして説明文を省く。テールは札の脇から画像の余白まで水平に出て、そこから円の縁へ向かう。
- **強調**: `focus` の説明以外は 30% の濃さにする（割り当ての一覧でホバーした操作を目立たせる）。

## 寄って映す（`zoom`）

- **寄り方**: 機種ごとにボタンの並ぶ広さと画像の中の位置が違うので、倍率は表から決める。顔（握りを除く見えるボタンをすべて囲む枠、`face`）が枠の幅の `FACE_W`・高さの `FACE_H` に収まる最大の倍率にし、顔の中心を枠の横中央・高さ `FACE_Y` に置く。下の握りは枠の下で切れる。
- **動き**: 常に出ている円は無く、ダブルタップのリズム（`TAP_PERIOD` ごとに 2 つ）で波紋だけを出す。波紋はボタンを囲む円の大きさで透明から現れつつ内へ沈み（押す。`PRESS`・`PRESS_DEPTH`）、離すと外へ（半径の `RIPPLE_GROW` 倍だけ）広がって、終わり際に消える。`enter` を渡すと、その時刻から少し待って（`ENTER_DELAY`）下から上がりつつ現れ（`ENTER`、`ENTER_RISE`）、上がりきってから波紋を出す。動いている間は `design::drive::keep_drawing` で描き続ける。示す部品が画像に無ければ（Steam Frame の左手の X など）波紋は出さず、登場のあとは止まる。
- **溶かし**: `fade` の高さで、画像の上に地の色（`theme.background`）から透明へのグラデーションを重ねる。

## テスト

- 単体: `cargo test -p guide`（全機種の画像と表、既定の割り当てが部品に当たること、配置の重なりと円の有無、寄り方が顔を枠に収めること、ダブルタップの動き）。
- 実 GPU の見た目: `cargo test -p ui --test gpu -- guide`。Testing のガイド画面と、全機種・左右の全ボタンを囲んだ一覧、全機種のセットアップの完了画面（`guide-done-*`）を `target/tmp/guide-*.bmp` に書き出す。
- 判定から表示まで: `cargo test -p testkit --test scenarios -- guide::`（レンダーモデル名を偽装したコントローラの検出、使用中の機種の表示、ホバーでの強調）。
