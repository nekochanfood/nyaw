# crates/overlay

層: 4 オーバーレイ

パネル配置、デバイスへの取り付け（追従）・掴めないようにするロック（空間固定）、レーザーのヒット判定、ポインタイベント生成、パネルの掴み移動、親子関係（子は親に追従、子が開いている間の親は暗くなり（子のフェードと同じ時間で暗転・復帰）入力を受けない、親が閉じれば子も閉じる）、ナビゲーションバー（持ち主の地の下端の左右中央に、バーの上端の左右中央を合わせて毎 tick 置く。頭からの距離に比例して大きくし見た目の大きさを保つ。持ち主と一緒に閉じ・暗くなり、掴むと持ち主を掴む）、空間全体の暗転（`set_veil` したパネルが出ている間、黒い幕を毎 tick 頭の正面 50m（描画順 0。パネルは 1 から。描画順でも奥行きでもパネルより奥）に置き、パネルの出入りと同じ動きで暗転・復帰する。暗転している間はゲームへの入力も止める（`VrBackend::block_app_input`。OpenVR は SteamVR Input のアクションセットをオーバーレイの全体優先度で更新して入力を奪う（SteamVR のメニューが開いている間は奪わない）。この仕組みは SteamVR の開発者向け設定「Experimental overlay input overrides」（`steamvr/globalActionSetPriority`）が無効だと働かないので、最初に止めるときに無効なら有効にする。OpenXR・novr は何もしない）） （パネルの表示アニメーションと影の余白は `design` の GPUI を持たない部分＝値と `design::animation` から引く。`default-features = false`）

## 主な公開 API

`Overlay::{add_panel, attach, detach, set_locked, set_parent, set_hidden, set_nav_bar, set_veil, set_resolution, tick, submit}`, `Parent { panel, carry, dismiss, hud, docked, slot }`（`dismiss` = 子の外側を押すと子を閉じる。ブロックしたい子だけ `PanelSpec::without_dismiss`。`docked` = 親に固定した子で、親を暗くせず入力も止めない。`slot` = 親の上端の上に、子のナビゲーションバーの分を空けて、親から見て指定の角度だけ起こして立てる（子の大きさが変わっても毎 tick 置き直す）。`slot` にはめる・外すときは `Motion::Move` で滑らかに動き、外すと離れたと分かるよう面の向きへ 0.2m 手前に出る。固定した組（バー・はめた画面・キーボード）はどれを掴んでも上にはめた画面を掴んだことにし（頭に向けて垂直に立て、ほかは付いてくる。はめた画面が無ければ根元の `grab_anchor`（ダッシュボードは画面が立つ所）を基準にする）、はめた画面は下端を動かさずに大きさを変え、入れ替わりで消える（閉じる・隠す）ときはフェードアウトしながら 0.15m 後ろへ下がる（重なってちらつかないように。隠れたものは外しても手前へ出さない）。ポップオーバー（固定して外側で閉じる子）は前面の描画順。固定した子のナビゲーションバーは頭からの距離で大きさを変えない）, `Overlay::set_resizable`（掴んで大きさを変えられるか。掴んだものと動かすものが違うときも変えない）, `Overlay::set_grab_anchor`（掴んだとき手で動かして頭へ向ける基準。`PanelSpec::grab_anchor`）, `Overlay::set_hidden`（閉じずに隠す。フェードアウトし、入力・暗転の対象外。子も一緒に隠れる）, `Panel::pixel_to_world`, `UiEvent`, `PointerEvent`

## パネルの表示アニメーション

- VR パネルは中央をアンカーに、サイズ 75%→100%・不透明度 0%→100% で表示し（200ms）、閉じると逆方向に消える（150ms）。
- `animato` で実時間を使って進め、最初の描画が届いてから開始する。途中で閉じても現在の状態から消え、入力は即座に止まる。
- `PanelSpec::without_animation()` で無効化。作成後は `PanelCommand::Animated(false)`、Testing の Animation ボタンでも切り替えられる。レーザー・カーソル・ドラッグ表示は既定で無効。
- フェードは OpenVR の `SetOverlayAlpha`、OpenXR の `XR_KHR_composition_layer_color_scale_bias` で合成し、再生中の画像の再転送を避ける。不透明度 API の無いランタイムだけ、加工前の描画を保持して RGBA を減衰させて転送する。デスクトップの OS ウィンドウは対象外。
- `cargo test -p overlay --test animation_runtime -- --ignored` で起動済み SteamVR でアニメーション画像を検証（HMD 必須）。
