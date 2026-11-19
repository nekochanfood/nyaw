# crates/subsystems/keyboard

層: 1 Subsystems

仮想キーボードの配列定義データ（xkeyboard-config）、キートップの文字マッピング、および修飾キー・トグル状態の管理を担当する。OS に対する直接の入力注入は行わない。

## キーボード機能の責務分離

仮想キーボード関連の処理は、関心事に応じて以下の 3 つの階層に分離されている。

1. **配列データと文字解決 (`crates/subsystems/keyboard`)**:
   - xkeyboard-config 由来の配列データベース（`Database`）を保持する。
   - 押下されたキースキャンコードから文字および修飾キー状態（`Stroke`）を算出する純粋な計算処理を担う。
   - 配列定義ファイルの走査にはディスク I/O を伴うため、UI 起動時にバックグラウンドスレッドで先行ロード（`preload`）してメモリ上に保持する。
2. **OS 入力注入と IME 監視 (`crates/subsystems/os::keyboard`)**:
   - OS 側で前面化されているアプリケーションのキーボード配列や IME 状態を監視する（`Watcher`）。
   - カーネルからの要求（`PanelCommand::TypeToOs`）に基づき、仮想キーストロークを OS 側へ直接注入する。
3. **キーボード UI とルーティング (`crates/ui::keyboard`)**:
   - 仮想キーボードパネル（`KeyboardView`）の描画と、入力先パネルへのルーティング（`InputRouting`）を行う。
   - GPUI 内部のテキスト入力欄への配送（`focus_input`）と、デスクトップ配信パネルに対する OS キーボード呼び出し（`toggle_os_keyboard`）を調停する。

## 主な公開 API

```rust
Database;        // 配列データベース（xkeyboard-config の保持）
Keyboard;        // キーボードインスタンスとレイアウト状態
Layout;          // 配列定義（キーの位置とサイズ）
Stroke;          // 打鍵結果（生成文字、修飾キー状態）
KeyboardType;    // キーボード種別（Full, Tenkeyless, Compact）
```
