# crates/animation

層: 4 UI（横断部品）

動きの責務。用途 `Motion`（長さとイージングを決める。長さは design の `tokens.json` の `duration-ms`）、毎フレーム進める `Animation`、始めた時刻と今から値を引く `Transition`、ボタンの滑り込みの `Pair` / `Slide`。GPUI にも `design` にも依存しない（境界は `tests/boundaries.rs`）。時刻を読む・再描画を要求する・何を動かすか決めるのは呼び出し側（`design` の部品、`overlay`）。イージングの式はここにだけ書く

## 主な公開 API

`Motion::{duration_ms, ease}`, `Animation::{new, advance, value, is_done}`, `Transition::{rest, go, at}`, `Pair`, `Slide`, `duration_ms`
