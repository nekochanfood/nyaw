# AGENT.md

本リポジトリは nyaw のモノレポである。この文書はリポジトリ全体の構成と共有の規約だけを扱う。各パッケージの設計と規約は、それぞれの `AGENT.md` に記載する。

## 構成

| 場所 | 内容 | ドキュメント |
|---|---|---|
| `apps/nyaw/` | VR オーバーレイアプリ（Cargo ワークスペース） | [`apps/nyaw/AGENT.md`](apps/nyaw/AGENT.md) |
| `apps/web/` | ウェブサイト（TanStack Start、Cloudflare Workers） | [`apps/web/AGENT.md`](apps/web/AGENT.md) |
| `assets/` | ロゴとアイコン（パッケージ間で共有する） | — |
| `.github/` | CI、リリース、Issue テンプレート | — |

パッケージは `apps/` に置き、固有の話題はそのパッケージの中に閉じる。

## 共有する資産

- **ロゴとアイコン**: ルートの `assets/` を正本とする。
- **デザイントークンとフォント**: アプリの `apps/nyaw/crates/design/`（`tokens.json`、`fonts/`）を正本とする。
- 他のパッケージは複製をコミットせず、直接参照するか、ビルド時に生成する。

## pnpm と Cargo

パッケージ管理には pnpm を使う。版はルート `package.json` の `packageManager` で固定する。

- **依存の取得**: `pnpm-workspace.yaml` の `cargo.enabled` により、`pnpm install` は Cargo の依存も取得する。pnpm の実験的な機能であり、設定や書き出す配置は pnpm の更新で変わる可能性がある（[pnpm.io/cargo](https://pnpm.io/cargo)）。
- **ビルド**: コンパイルは cargo が行う。cargo は `.cargo/config.toml` をカレントディレクトリから探すため、`apps/nyaw/` で実行する。
- **生成物**: `pnpm install` は取得したソースを `apps/nyaw/.pnpm/` に置き（コミットしない）、`apps/nyaw/.cargo/config.toml` の `pnpm-managed cargo sources` の区間から読み込ませる。この区間は pnpm が管理するため手で編集しない。`.pnpm/` がなければ（`pnpm install` をしていない CI など）、cargo は通常どおりレジストリから取得する。この読み込みの指定には Cargo 1.95 以降が必要である。
- **ロックファイル**: クレートは `Cargo.lock`、npm パッケージは `pnpm-lock.yaml` に記録する。
- **vendor の除外**: `packages` の `!**/vendor/**` を外すと、pnpm は `apps/nyaw/vendor/` のクレートを独立したプロジェクトとみなし、`Cargo.lock` と `.cargo/` を書き込む。公式ドキュメントにない挙動のため、pnpm を更新したら確かめる。

## CI とリリース

- **名前**: アプリ専用とウェブサイト専用のワークフロー名は、`nyaw`・`web` で始める。画面では「ワークフロー名 / ジョブ名」と並ぶため、ジョブ名には対象を繰り返さない。
- **変更の判定**: `changes.yml`が変更されたファイルから対象を判定し、`ci.yml`（アプリ）と `web.yml`（ウェブサイト。Cargo の依存は取得しない）は関係するジョブだけを実行する。対象外のジョブは「スキップ」になる。判定の規則は `.github/scripts/changes.sh` にあり、変えたら `bash .github/scripts/changes.test.sh` で確かめる。パッケージやそれが読むファイルを増やしたら、規則にも足す（足し忘れると検査されない）。
- **CodeQL**: PR の Rust の解析だけを、アプリに関係する変更があるときに限る。
- **`main` への push による `release.yml`**: `apps/nyaw/`・`assets/`・`release.yml` の変更だけで実行する。
- **リリース**: バージョン（CalVer）、タグ、GitHub Releases はアプリ専用とする（アプリの更新確認が参照する）。ウェブサイトだけの PR には `web` ラベルを付け、リリースノートから除外する（`.github/release.yml`）。
- **ビルド番号**: アプリのビルド番号はリポジトリ全体のコミット数のため、他のパッケージのコミットでも増える（大小比較には影響しない）。

## 規約

- コミットは Conventional Commits の形式とする。
- `main` への変更は Pull Request を通し、CI がすべて成功してからマージする。`main` は線形の履歴を必須とする。
