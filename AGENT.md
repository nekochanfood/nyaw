# AGENT.md

nyaw は GPUI で GUI を描く VR オーバーレイシステム。Cargo ワークスペースで、層ごとに 1 クレート。

## 層構成（下 → 上）

| 層 | クレート | 責務 | 主な公開 API |
|---|---|---|---|
| 0 OS | `crates/os` | Windows/Linux の差を吸収（パス、環境変数、ランタイム検出）。`#[cfg]` はここだけに書く | `config_dir`, `openvr_paths_file`, `openxr_runtime_manifest` |
| 1 VR | `crates/vr` | OpenVR / OpenXR の差を吸収。上位層は `Box<dyn VrBackend>` しか持たない | `connect`, `VrBackend`, `Pose`, `Frame`, `RawController`, `mock::MockBackend`（`test-support` フィーチャー時のみ） |
| 2 入力 | `crates/input` | コントローラ機種の差を吸収（ドライバ的役割）。プロファイル表で生入力 → 抽象アクション | `map(&RawController) -> ControllerState`, `source(profile, Action)`（逆引き） |
| 3 オーバーレイ | `crates/overlay` | パネル配置、レーザーのヒット判定、ポインタイベント生成、パネルの掴み移動 | `Overlay::{add_panel, tick, submit}`, `UiEvent`, `PointerEvent` |
| 4 UI | `crates/ui` | GPUI フロントエンド。ポインタイベントを GPUI のマウス入力に変換してビューへ流す | `run`, `pump`（1フレーム分。本番と testkit 共通）, `PointerBridge`, ビュー（`Home`） |
| - | `crates/testkit` | 全層を丸ごと持つ Playwright 風 E2E テスト基盤。開発専用 | `Kit::{click, hover, press, drag_panel, hand, profile, read}` |
| - | `src/main.rs` | ブートローダー。ランタイム選択 → `ui::run` に引き渡すだけ | — |

依存は**必ず下向き**、かつ原則として 1 つ下の層まで（`os ← vr ← input ← overlay ← ui ← main`）。overlay は `vr` の型（`Pose` 等）を共通語彙として使ってよい。

## 1 フレームの流れ

```
ui::run（GPUI のイベントループ上で約 90Hz）/ testkit の Kit::step
 └ ui::pump → overlay.tick(vr)
     ├ vr.poll()            → Frame { hmd, controllers: Vec<RawController> }
     ├ input::map(raw)      → ControllerState { aim, select, grab, menu, scroll }
     ├ レーザー × パネル判定 → Vec<UiEvent { panel, hand, PointerEvent }>
     └ vr.set_overlay_pose  （掴み移動を反映）
 └ PointerBridge::translate → gpui::PlatformInput → window.dispatch_event
```

## 規約

- 語彙は OpenXR に合わせる: プロファイル `"valve/index_controller"`、入力 `"trigger/value"`。OpenVR バックエンドが翻訳する。
- 座標: トラッキング空間はメートル・前方 -Z。パネル表面はローカル +Z。パネルのピクセル座標は左上原点。
- 新しいコントローラ対応 = `crates/input` の `PROFILES` に 1 行追加。上位層でプロファイル名を見ない。
- 新しい VR ランタイム = `crates/vr` に `VrBackend` 実装を追加し `connect` に登録。
- `CODE_OF_CONDUCT.md` に従う: コメントは日本語、Conventional Commits、CalVer（現在 `2026.0.1`、`[workspace.package]` で一括管理）、単体テスト必須、層をまたぐものは結合テスト。
- `// ponytail:` コメントは意図的な簡略化。限界と次の一手が書いてある。

## テスト

`cargo test --workspace`。CI は `cargo fmt --check` と `cargo clippy --workspace --all-targets -D warnings`。

- **単体テスト**: 各クレート内の `#[cfg(test)]`。その層の範囲だけで完結させる。実機の代わりは `vr` の `test-support` フィーチャーで出る `vr::mock::MockBackend`。単体テストから testkit は使わない（依存が循環する）。
- **ユーザー操作のシナリオ（E2E 風）**: `crates/testkit/tests/` に置き、`Kit` 経由で書く。要素は GPUI の `.debug_selector(|| "名前".into())` で指定する。プロファイル名や座標計算はテストに書かない。

```rust
#[gpui::test]
fn click_button(cx: &mut TestAppContext) {
    let mut kit = Kit::new(cx, |_, _| Home::default());
    kit.hand(Hand::Left).profile("oculus/touch_controller").click("click-me");
    assert_eq!(kit.read(|h| h.clicks), 1);
}
```

## 未実装（土台のみ）

- `vr` の OpenVR / OpenXR 実 FFI（`connect` は `NotImplemented` を返す）。
- GPUI の描画結果をオーバーレイのテクスチャに送る経路（現在はデスクトップウィンドウに描画）。
- Windows での OpenXR ランタイム検出（レジストリ）。
- `gpui` は crates.io 版では `Window::dispatch_event` を外部から呼べないため、Zed の git リビジョンに固定している。
