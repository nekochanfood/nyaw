# crates/subsystems/input

層: 1 Subsystems

VR コントローラの機種ごとのハードウェア差異を吸収し、生入力を正規化された共通アクションへ変換する。

プロファイル定義テーブル（`PROFILES`）に基づくハードウェア入力の抽象化に加え、コントローラを持たない手に対する骨格ハンドトラッキングからの仮想コントローラ生成を担当する。

## 骨格ハンドトラッキング（仮想コントローラ）

コントローラを持たない素手操作時、または光学式ハンドトラッキング環境においては、骨格データから仮想コントローラ（`hand_controller`、プロファイル名 `ext/hand_interaction_ext`）を生成して入力イベントを合成する。

- **ピンチによるクリック**: 親指と人差し指の先端を接触させる（つまむ）動作をクリックとして判定する。
- **指の屈曲による把持（掴み）**: 中指・薬指・小指を手のひら側へ握り込む動作（指関節の曲がり角度から算出）をグリップ押下として判定する。
- **ショートカットジェスチャー**: 左手の手のひらを顔に向け、親指と小指を 0.5 秒間（`kernel::HAND_LONG_PRESS`）接触させ続けることで、コントローラの左手 X ボタン二回押しと同等のショートカット入力を発行する。
- **光線照射の基準**: レーザー光線は親指と人差し指の付け根の中間を起点とし、手首から中指の付け根を結ぶ方向（`HandSkeleton::grip`）へ照射する。手首の角度によって向きを制御し、指先の微小な動きによって光線がブレることを防ぐ。

## 手振れ補正とデバイス切り替え

- **手振れ補正フィルタ (`HandFilter`)**: One Euro フィルタを応用した動的補正を適用する。手を静止させている間は強くブレを抑え、素早く振った際は遅延なく追従する。ピンチ操作中およびクリックホールド中は補正をさらに強化し、クリック時の指の反動によるポインタズレを防止する。
- **入力ソースの切り替え（ヒステリシス）**: コントローラとハンドトラッキングの両方が検出されている状態では、切り替えのバタつき（チャタリング）を防ぐため、18 フレーム連続で同一状態が維持された場合にのみ入力元を切り替える。ただし、コントローラの一時的なトラッキングロストから復帰した際は、即座にコントローラ入力を優先する。
- **手首パネルの安定追従**: 腕時計パネル（wrist）のアタッチは、仮想コントローラであってもランタイム側のデバイス姿勢情報に相対配置されるため、ダッシュボード開閉に伴う入力優先度切り替え時にもパネル位置の跳びは発生しない。

## 主な公開 API

```rust
// 生入力から正規化コントローラ状態への変換
map(raw: &RawController, prev: Option<&ControllerState>) -> ControllerState;

// 機種プロファイルからの逆引き
source(profile: &str, action: Action) -> Option<InputSource>;
scroll_source(profile: &str) -> Option<InputSource>;

// 握っているコントローラの機種と割り当て（操作ガイド用。ControllerState::controller）
Controller::identify(raw) -> Option<Controller>;   // 骨格だけの手は None
Controller::of(model);                             // 一覧表示用: 機種の既定の割り当て
controller.parts(Control::Grab, hand) -> Vec<&str>; // 操作を起こす部品（画像のボタン名: trigger, grip, b …）
Model::{ALL, name, render_model, from_render_model};
```

## 機種の判定（操作ガイド）

コントローラの外観（[`guide`](../../guide/AGENT.md) が画像とボタンの位置を選ぶ鍵）を `Model` で表す。名前は SteamVR のレンダーモデルに揃え、表は `MODELS`。

- **判定の順**: ランタイムが報告したレンダーモデル名（`RawController::model`。SteamVR の `Prop_RenderModelName_String`。`{ドライバ名}` の前置きと大文字小文字は無視し、`_left` / `_right` の付いた系列と別名に合わせる）→ 割り当て（プロファイル。Index・Vive・Steam Frame）→ **Quest Touch Plus**（Quest 3 / 3S。最も多く使われている形のため、分からないときの代わりにする）。OpenXR と novr はレンダーモデル名を報告しない。
- **部品**: 操作ごとの部品は、実際に読んでいる割り当て（`PROFILES`）の入力名の先頭（`squeeze` は `grip`、`select` は `trigger` と読み替える）。機種に無い部品も返すので（Touch のメニューは ≡・Y・B）、表示側が画像にある部品だけを使う。Steam Frame は OpenXR にプロファイルが無いので nyaw の名前（`FRAME_PROFILE`、`valve/frame_controller`）で、SteamVR の既定のバインド（`bindings_frame_controller.json`）から読む。ショートカットはどの機種にもある（Vive は左のトラックパッドのクリック、Frame は左の十字キーの下。`every_controller_has_a_shortcut`）。PICO のように SteamVR に既定のバインドが無く簡易コントローラとして読んでいる機種は、その割り当てのまま表示する（つかむが無い、など）。
