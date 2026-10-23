# crates/subsystems/media

層: 1 Subsystems

再生中のメディア（音楽・動画）の情報と操作。Windows は GSMTC（`GlobalSystemMediaTransportControlsSessionManager`。アプリの名前とロゴは `AppInfo`）。ワーカーが 1 秒ごとに問い合わせ、ジャケットの縮小・背景用のぼかし・背景の明るさ（`light`）もワーカーで求める。メインスレッドは最新の結果を受け取るだけ

## 主な公開 API

`Media::{start, now, send}`, `NowPlaying`（`position_now`）, `App`, `Controls`, `Command`, `Image`
