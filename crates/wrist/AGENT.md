# crates/wrist

層: 5 画面

常駐パネル。左手首に腕時計のように取り付き（掴めない）、丸い文字盤（縁に目盛りと秒の点）に日付・時刻。常駐なので、X ボタンの二回連続押し（`PanelStatus::double_press` の回数が増える）でダッシュボードを開く（2 回目からは隠す / 出し直す。閉じない）。ダッシュボードから開くデバッグ画面・設定画面は main が渡す（`wrist::spec(debug, settings)`。今は Testing とその設定画面。画面どうしは直接依存しない）

## 主な公開 API

`wrist::spec(debug, settings)`, `Wrist`, `WATCH_OFFSET`
