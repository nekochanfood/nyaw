# crates/logger

層: -

logfmt 形式の構造化ロガー（横断）。整形・レベル・出力先 `Sink` の抽象と、`log` クレート（gpui 等）からの橋渡し

## 主な公開 API

`trace!/debug!/info!/warn!/error!`, `Logger`, `Sink`, `StderrSink`, `FileSink`, `create_rotated`, `init`, `format`
