# crates/profiler

層: 4 UI（横断部品）

パフォーマンス計測時にのみ動作する軽量プロファイラを提供する。起動引数 `--profile` の指定、Testing 画面の切り替えボタン、または `PanelControl::profiling` により有効化される。

## 計測フェーズと出力仕様

UI 層のフレームループと連動し、1 フレーム内の各フェーズにおける所要時間を記録する。

- **計測フェーズ (`Stages`)**: `wait`（待機）、`input`（入力変換）、`draw`（GPUI描画）、`render`（レンダリング）、`upload`（GPU転送）、`overlay`（姿勢計算）、`compositor`（novr画面合成）、および `profiler`（プロファイラ自身の描画時間）。
- **リアルタイムグラフ表示 (`ProfilerView`)**: プロファイラパネル上に FPS、フレーム所要時間、CPU/GPU 処理時間、およびフレームドロップ率を折れ線グラフで可視化する。ナビゲーションバーの閉じるボタンを押下すると計測自体が停止する。
- **CSV ダンプ**: `--profile` オプション付きで起動された場合、1 フレームごとに 1 行の CSV データ（保存先: `os::log_dir()/profile.csv`）を出力する。

## 主な公開 API

```rust
Profiler::{
    new,             // プロファイラインスタンスの生成
    with_budget,     // 目標フレーム時間予算の設定
    record,          // 各フェーズ時間の記録
    add_self_time,   // プロファイラ自身の処理時間の加算
    summary,         // 集計情報の取得
};
ProfilerView;        // GPUI 表示用ビュー
Stages;              // 各処理フェーズの定義
CSV_HEADER;          // CSV 出力用ヘッダー文字列
```
