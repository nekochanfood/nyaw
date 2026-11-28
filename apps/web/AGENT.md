# apps/web

nyaw のウェブサイト。リポジトリ全体の構成と共有の規約はルートの [`AGENT.md`](../../AGENT.md) を参照する。

## 構成

- TanStack Start（React）を Cloudflare Workers で動かす（`vite.config.ts`、`wrangler.jsonc`）。サーバーの入口は `src/server.ts`。
- スタイルは Tailwind CSS、UI の部品は Base UI（`@base-ui/react`）、アイコンは Phosphor Icons を使う。
- 絵文字は `ui/Twemoji` で [jdecked/twemoji](https://github.com/jdecked/twemoji) の画像にして表示する（パーサーは `src/twemoji/parse.ts`）。画像は版を固定して jsDelivr から取得する。グラフィックは CC-BY 4.0。
- 動きは anime.js で付ける（ポップアップの出入り、ページの出入り、ヘッダーの項目の登場）。スクロールは lenis で滑らかにする。動きを減らす設定（`prefers-reduced-motion`）では、どちらも使わない。
- ページの出入りは `PageTransition` が動かす。読み込み画面 → ページのフェードインの順に進め、遷移のときはフェードアウトしてから次のページの描画が落ち着くのを待つ（`src/motion/stage.ts` の `waitSettled`）。動く要素は動きが始まるまで CSS で隠す（`.motion`）。
- 整形と静的解析は [Oxc](https://oxc.rs/) のツール（oxfmt、oxlint。それぞれ別のパッケージ）を使う。
- テストは Vitest（`src/**/*.test.ts`）と Playwright（`e2e/`）を使う。
- インポートは別名で書く（`@/` は `src/`、`@locale/` は `locale/`、`@assets/` はルートの `assets/`）。別名は `alias.ts` と `tsconfig.json` の `paths` で揃える。

## ページと言語

- ページは言語ごとのパス（`/en/…`、`/ja/…`）に置く。言語のないパス（`/`、`/download` など）は、`src/server/localeRedirect.ts` が cookie（言語メニューで選んだ言語）か Accept-Language で選んだ言語へ 308 で転送する。
- 文言は `locale/{en,ja}.json` に置き、`useT(名前空間)` で引く。英語と日本語のキーと変数が揃っていること、英語がイギリス英語の綴りであること、日本語の和欧文の間隔はテストが確かめる。文言はアプリの翻訳のガイドライン（`apps/nyaw/crates/i18n/locales/*/GUIDELINES.md`）に従う。
- ページのタイトルは「（ページ名） / nyaw」。`head` は `src/seo.ts` の `seo` で、canonical、言語ごとの hreflang、OGP（画像を除く）を出す。robots.txt と sitemap.xml は `src/server/seoFiles.ts` が返す。

## 見た目

- 色は Catppuccin（ライトは Latte、ダークはロゴと同じ Frappé）。`src/styles.css` の `--ctp-*` に使う色だけを置き、ページの役割（`card`、`frame`、`neutral-text` など）に割り当てる。アクセントは Lavender（`ctp-lavender`）を小さな面積に置く。
- テーマは保存したもの（`localStorage`）か OS の設定に従う。`src/theme.ts` の `themeScript` が描画の前に `<html data-theme>` を付ける。切り替えは画面全体のクロスフェード（View Transitions API）で見せ、要素ごとの色のトランジションは使わない（継承した色の補間が毎フレーム作り直され、文字やアイコンがチカチカする）。
- 角はスーパーエリプス（`corner-shape`）。対応するブラウザでは、トークンの角丸に `--corner-scale` を掛けて広げる。中に項目を並べるパネルは、外側の半径を「中の半径＋余白」にして角を同心にする。
- ページは帯（外枠）で囲み、その内側に本文のカードを置く。ヘッダーは上辺の帯に、フッターはカードの下の帯に載る。帯の寸法（`--frame-*`）、帯の色（`frame`。Catppuccin の Crust）、重なり順（`--layer-*`）は `src/styles.css` の `:root` にまとめ、ほかの場所に数値を書かない。帯の内側に置くものの幅は `frame-x` でそろえる。
- 見た目のクラスの組み合わせは `src/ui/variants.ts` にまとめる。同じ組み合わせが 2 か所以上に出てきたら、`variants.ts` に名前を付けて足す。
- UI の状態の切り替え（ポップアップの出入り、パネルの切り替え）の時間は、トークンの時間（`src/motion/duration.ts` の `duration`）を使う。ページの演出（ページの出入り、ヘッダーの項目の登場）の時間は、それぞれの動きが持つ。

## アプリのキャプチャ

アプリの GPU テストは、パネルのキャプチャを `public/scene/<テーマ>-<言語>/<場面>/` に書き出す。今のページはキャプチャを使っていない（`public/scene/` はない）。ページの絵に使うときは、`apps/nyaw/` で次を実行して書き出す（Windows と GPU が要る）。

```sh
cargo test -p ui --test gpu -- --ignored site
```

OGP の画像はまだない。用意したら、`src/seo.ts` の `seo` に og:image と twitter:image を足す。

## コマンド

このディレクトリで実行する（ルートからは `pnpm --filter web <名前>`）。

| コマンド                                     | 内容                                         |
| -------------------------------------------- | -------------------------------------------- |
| `pnpm dev`                                   | 開発サーバー                                 |
| `pnpm build` / `pnpm preview`                | 本番ビルドと、その手元での確認               |
| `pnpm fmt` / `pnpm fmt:check`                | 整形と、その確認                             |
| `pnpm lint` / `pnpm typecheck` / `pnpm test` | 静的解析、型検査、単体テスト                 |
| `pnpm e2e`                                   | E2E テスト（ビルドしたサイトを配信して実行） |
| `pnpm run deploy`                            | Cloudflare Workers へのデプロイ              |

E2E テストを初めて実行するときは、`pnpm exec playwright install chromium webkit` でブラウザを取得する。

CI（`.github/workflows/web.yml`）は整形の確認、静的解析、型検査、単体テスト、ビルド、E2E テストを、それぞれ別のジョブで並列に実行する。実行するのは、ドラフトでない PR の更新と `main` への push だけである。

## デザイントークン

`scripts/tokens.mjs` がアプリの `tokens.json` から、角丸と時間を `src/tokens.css` に書き出す（`dev`・`build`・`typecheck` が実行する。コミットしない）。

- 色はアプリと共有しない。色は `src/styles.css` が Catppuccin の値で持ち、役割の名前で Tailwind に登録する（`bg-card`、`text-neutral-text` など）。Tailwind の既定の色は使わない。ライトとダークは `light-dark()` で、`color-scheme`（テーマ）に従う。
- 余白は Tailwind の既定（4px の倍数）を使う。文字の大きさなど、アプリの画面向けの値は共有しない。
- ウェブサイトの色の組み合わせは、コントラストの検査を受けない（アプリのビルドが検査するのは `tokens.json` の色だけ）。

`src/routeTree.gen.ts` は TanStack Router が生成する。手で編集せず、コミットする。
