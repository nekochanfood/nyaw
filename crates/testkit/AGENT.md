# crates/testkit

層: T テスト

Playwright 風 E2E テスト基盤。開発専用。ランタイムごとに対等なキット

## 主な公開 API

`Driver::{click, hover, down, up, drag, scroll, read, color_at}`（全キット共通）, `on_every_runtime!`, `NoVrKit::{screenshot, on_screen, color_on_screen, move_mouse, press, release, stick, select_panel, type_keys}`, `DesktopKit::with_backend`, `VrKit::{panel, add_panel, press, drag_panel, move_hand, hand, profile, point_at}`, `runtimes::{install, launch, Launched::connect}`（実ランタイムの用意・起動・後片付け）

## 実ランタイムでの E2E

- **実ランタイムでの E2E**: `crates/testkit/tests/runtimes.rs`（`harness = false`）。`testkit::runtimes` がランタイムを起動し（Playwright の `webServer` に当たる）、繋いでオーバーレイへ送り、Testing をマウスで操作して（取り付け・ロック）その描画を `DesktopKit::with_backend`（パネルのウィンドウへマウスを直接送るキット）で実ランタイムへ送り、OpenVR はランタイムから読み戻して（光ったボタンまで）確かめ、Drop で自分が起動したプロセスを止めて消えたことまで確かめる。起動に数秒かかり画面にも出るので `cargo test -p testkit --test runtimes -- --ignored [monado|steamvr]` のときだけ流す。入っていないランタイムと、すでに動いているランタイム（別の設定で動いているかもしれず、止めると持ち主が困る）はスキップする。
  - **ランタイムを入れる**: `cargo runtimes`（`.cargo/config.toml` の別名。`npx playwright install` に当たるウィザード。`list` / `install [名前] [--dry-run]` / `uninstall [名前]`）。入れる先は `NYAW_RUNTIMES_PATH`、無ければ `%LOCALAPPDATA%\nyaw\runtimes`。
  - **Monado**: Windows 版は公式の配布物が無いので、Monado の GitLab CI の `windows` ジョブの成果物（main の最新。`install/` に `monado-service.exe` と `openxr_monado.json`）を入れる。`XRT_COMPOSITOR_COMPUTE=false` で起動し（コンピュート合成でクアッドを出すと落ちる）、`XR_RUNTIME_JSON` でマニフェストを指す。模擬 HMD（simulated）の画面にパネルが映る。読み戻しの手段は無い。ログは `<入れる先>/logs/monado-service.log`。
  - **SteamVR**: Steam の管理下なので入れない（`steam://install/250820` を開くだけ）。HMD が無くても動くよう、null ドライバを有効にした `steamvr.vrsettings` だけを置いた一時フォルダを `VR_CONFIG_PATH` で指す（普段の設定には触らない）。最初に繋いだクライアントが SteamVR を起動する。null ドライバだと `steamvr_room_setup` が立ち上がって vrserver を起動し直すので、後片付けは静かになるまで止め続ける。
