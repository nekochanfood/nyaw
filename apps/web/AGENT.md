# apps/web

nyaw のウェブサイト。リポジトリ全体の構成と共有の規約はルートの [`AGENT.md`](../../AGENT.md) を参照する。

## 構成

- TanStack Start（React）を Cloudflare Workers で動かす（`vite.config.ts`、`wrangler.jsonc`）。
- スタイルは Tailwind CSS、UI の部品は Base UI（`@base-ui/react`）を使う。
- 整形と静的解析は [Oxc](https://oxc.rs/) のツール（oxfmt、oxlint。それぞれ別のパッケージ）を使う。
- テストは Vitest を使う。

## コマンド

このディレクトリで実行する（ルートからは `pnpm --filter web <名前>`）。

| コマンド                                     | 内容                            |
| -------------------------------------------- | ------------------------------- |
| `pnpm dev`                                   | 開発サーバー                    |
| `pnpm build` / `pnpm preview`                | 本番ビルドと、その手元での確認  |
| `pnpm fmt` / `pnpm fmt:check`                | 整形と、その確認                |
| `pnpm lint` / `pnpm typecheck` / `pnpm test` | 静的解析、型検査、テスト        |
| `pnpm run deploy`                            | Cloudflare Workers へのデプロイ |

CI（`.github/workflows/web.yml`）は整形の確認、静的解析、型検査、テスト、ビルドを、それぞれ別のジョブで並列に実行する。実行するのは、ドラフトでない PR の更新と `main` への push だけである。

## デザイントークン

`scripts/tokens.mjs` がアプリの `tokens.json` から `src/tokens.css` を生成する（`dev`・`build`・`typecheck` が実行する。コミットしない）。

- 色は役割の名前で Tailwind に登録し（`bg-card`、`text-neutral-text` など）、Tailwind の既定の色は使わない。ライトとダークは `light-dark()` で OS の設定に従う。
- 角丸と時間も共有する。余白は Tailwind の既定（4px の倍数）を使う。文字の大きさなど、アプリの画面向けの値は共有しない。
- コントラストの検査はアプリのビルドが `tokens.json` に定義された組み合わせに対して行う。ウェブサイト独自の組み合わせは検査されない。
- 参照の解決規則は `apps/nyaw/crates/design/build.rs` と重複する。解決できない参照があると生成は失敗する。

`src/routeTree.gen.ts` は TanStack Router が生成する。手で編集せず、コミットする。
