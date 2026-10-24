# crates/wrist

層: 5 画面

常駐パネル。利き手（`config.toml` の `dominant_hand`、既定 `right`）でない方の手首に腕時計のように取り付き（掴めない）、丸い文字盤（縁に目盛りと秒の点）に日付・時刻。常駐なので、X ボタンの二回連続押し（`PanelStatus::double_press` の回数が増える）でダッシュボードを開く（2 回目からは隠す / 出し直す。閉じない）。ダッシュボードから開くデバッグ画面・設定画面は main が渡す（`wrist::spec(hand, debug, settings)`。`hand` は main が `Settings::dominant_hand.off_hand()` で渡す。今は Testing とその設定画面。画面どうしは直接依存しない）

## 主な公開 API

`wrist::spec(hand, debug, settings)`, `Wrist`, `WATCH_OFFSET`
