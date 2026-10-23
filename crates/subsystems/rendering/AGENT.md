# crates/subsystems/rendering

層: 1 Subsystems

GPUI の非表示描画先・読み戻し・描画密度と OS 差、ランタイムへの画像転送・不透明度 / 相対追従の能力差・描画資源の保持と解放。UI は `kernel::rendering` 経由で使う

## 主な公開 API

`Surface::{new, store, present(opacity, dim), place, show, release}`, `gpui::{capture, read_back, open_window, set_render_lod}`

## 描画層（Issue #41）

- `rendering` は `overlay` の下、`vr` の上に入り、カーネルが `kernel::rendering` として公開する。UI から描画クレートへ直接依存しない。
- `overlay::Panel` が持つ `rendering::Surface` が描画資源と送信済み状態を保持する。配置・入力判定・アニメーションの目標と進行は overlay が決め、描画先へ送るときの能力差は Surface が吸収する。
- `ui::{capture, Frame, set_render_lod}` は rendering の再エクスポート。`ui::{open_panel_window, panel_window_options}` は UI の表示方針で、OS・描画ホスト固有の作成と読み戻しは `crates/subsystems/rendering` が担う。
- `cargo test -p rendering --all-features` は画素変換、アルファ対応 / 未対応、追従のフォールバック、解放後の更新抑止を検証する。実描画は既存の `gpu_capture` / `panel_lod` で描画層からカーネルまで通す。
