# nyaw (にゃう)
VR オーバーレイアプリ

## 開発・実行

必要なもの: Rust (edition 2024 対応の stable)、cmake と C++ コンパイラ（OpenVR / OpenXR のビルドに使う）。

```sh
cargo run                  # VR ランタイムを自動検出。無ければ novr（デスクトップのウィンドウ）で起動
cargo run -- --novr        # novr 固定
cargo run -- --openxr      # OpenXR 固定（--openvr で SteamVR）
NYAW_LOG=debug cargo run   # ログレベル変更（trace/debug/info/warn/error、既定 info）
```

ログは標準エラーと `%LOCALAPPDATA%\nyaw\logs\nyaw.log`（Linux は `$XDG_STATE_HOME/nyaw/logs`）に出る。

### テスト

```sh
cargo test --workspace     # novr 上の E2E を含む（実ランタイム不要）
cargo runtimes list        # 実ランタイム (Monado / SteamVR) の状態
cargo runtimes install     # Monado を導入するウィザード（SteamVR は Steam 側で入れる）
cargo test -p testkit --test runtimes   # 実ランタイムでの E2E
```

構成と設計は [AGENT.md](AGENT.md) を参照。
