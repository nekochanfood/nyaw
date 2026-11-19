# crates/logger

層: 0 基盤

logfmt 形式に対応した構造化ロガー基盤を提供する。

全層から直接参照できる唯一の横断的クレートであり、他の自前クレートには一切依存しない。標準の `log` クレート（GPUI 等の外部ライブラリ）の出力も本ロガーへ転送する。

## 主な機能と特徴

- **構造化出力マクロ**: キー・値ペアの出力をネイティブにサポートするマクロ群（`info!`, `warn!`, `error!` 等）を提供する。
- **出力フォーマット**: `ts=... level=... target=... msg=... key=value` の 1 行 1 件の logfmt 形式を採用する。
- **出力シンク (`Sink`)**: 標準エラー出力（`StderrSink`）およびファイル出力（`FileSink`）を抽象化する。
- **世代管理 (`create_rotated`)**: アプリケーション起動時に既存ログを `*-prev.log` に 1 世代のみ退避し、新規ログファイルを作成するローテーション機能を備える。

## 主な公開 API

```rust
// 構造化ログマクロ
logger::trace!(...);
logger::debug!(...);
logger::info!(...);
logger::warn!(...);
logger::error!(...);

// 初期化とシンク設定
init(sinks: Vec<Box<dyn Sink>>, level: LevelFilter);
Logger;
Sink;
StderrSink;
FileSink;
create_rotated(path);
```
