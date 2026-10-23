# crates/kernel

層: 3 カーネル

設定・フレーム進行・パネル操作を束ねる。最下部の Subsystems の上に位置し、UI・画面はカーネルだけを見る。設定は `subsystems::os::config_dir()/config.toml`（アプリ全体の設定はパネルの操作 `PanelCommand` と分け、`Kernel::set(Setting)` で変えて `Kernel::settings()` で読む。`theme = "dark" | "light"` は `Setting::Theme`、`language = "en" | "ja"` は `Setting::Language`、`pointer_smoothing`（レーザーの手ブレ抑え。0〜0.998、既定 `overlay::POINTER_SMOOTHING` = 上限の 0.998）は `Setting::PointerSmoothing`、`pointer_release_deg`（抑えたレーザーがこの角度だけ手から遅れると抑えが半分になる。0.5〜45°、既定 11.25°）は `Setting::PointerRelease`、`laser_length`（レーザーの見える長さ m。0.05〜1.5、既定 0.3）は `Setting::LaserLength` で切り替えると書き戻され、次回起動に残る。`ui::sync_settings` が `design::Mode` のグローバルに反映し、画面は `Theme::current(cx)` で引く）。OS への入力（配信のタッチ・仮想キーボードのキー）はどちらもカーネル経由（`PanelCommand::Stream` / `PanelCommand::TypeToOs`）。優先順位はコマンドライン > 設定ファイル（ログレベルは `NYAW_LOG` > 設定ファイル）

## 主な公開 API

`Kernel::{boot, with_backend, tick, submit, apply, status, set, settings, os_keyboard, add_panel, batteries, now_playing}`（メディアの見張りは `boot` だけが起こす）, `PanelCommand`（`Hide(bool)` で閉じずに隠す・出し直す、`Resizable(bool)`、`GrabAnchor(Option<Pose>)`、`Media(media::Command)`、`FocusSource`（配信しているウィンドウを OS で前に出す））, `PanelStatus`（`hidden`）, `Setting`, `Settings`, `config::{load, parse, path, Config}`, `subsystems`, `rendering` / `vr` / `overlay` の再エクスポート

## パネルの LOD

- `config.toml` の `lod_quality`: 0 = Off（常に LOD0）、1 = Low、2 = Mid（既定）、3 = High。`Kernel::panel_lod` が HMD とパネル中心の距離から決める。距離はパネルの密度（1m あたりの論理ピクセル）で換算し、基準（1000px/m）より粗いパネルは近くにあるものとして扱う（拡大すると LOD が下がり、ぼやけたまま残らない。Issue #51）。
- LOD0〜3 は縦横 1 / 1/2 / 1/4 / 1/8。切り替え距離は Low が 1 / 2 / 4 m、Mid が 2 / 4 / 8 m、High が 4 / 8 / 16 m。
- フレームレートも LOD で絞る。LOD0 は毎フレーム、LOD1〜3 は 2 / 4 / 8 フレームに 1 回だけ描き直す（`Kernel::panel_frame_due`。パネルごとにずらす）。間引いたフレームの入力・変化は次に描いてよいフレームで描く。
- GPUI は非表示ウィンドウの DPI と実ピクセルサイズを変える。カーネルの論理解像度と物理サイズは変えない。8 の倍数でないサイズは表示領域に最大 7 論理ピクセルの丸めがある。
- `cargo test -p ui --test panel_lod -- --ignored` で実描画と入力を確認し、`--off` を追加して無効時も確認する（Windows）。
- ストレステストは `NYAW_STRESS_LOD=0` と `2` で同じ負荷を比較できる。
