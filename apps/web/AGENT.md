# apps/web

nyaw のウェブサイト。リポジトリ全体の構成と共有の規約はルートの [`AGENT.md`](../../AGENT.md) を参照する。

## 構成

- TanStack Start（React）を Cloudflare Workers で動かす（`vite.config.ts`、`wrangler.jsonc`）。サーバーの入口は `src/server.ts`。
- スタイルは Tailwind CSS、UI の部品は Base UI（`@base-ui/react`）、アイコンは Phosphor Icons を使う。
- 動きは anime.js で付ける（ポップアップの出入り、スクロールに連動する動き）。スクロールは lenis で滑らかにする。動きを減らす設定（`prefers-reduced-motion`）では、どちらも使わない。
- 動きの順番は `src/motion/stage.ts` で決める。読み込み画面 → ページのフェードイン（`PageTransition`）→ ページの中の動き（ページ名の文字、スクロールで現れる要素）の順に進め、遷移のときはフェードアウトしてから次のページの描画が落ち着くのを待つ。動く要素は動きが始まるまで CSS で隠す（`.motion`）。
- 整形と静的解析は [Oxc](https://oxc.rs/) のツール（oxfmt、oxlint。それぞれ別のパッケージ）を使う。
- テストは Vitest（`src/**/*.test.ts`）と Playwright（`e2e/`）を使う。
- インポートは別名で書く（`@/` は `src/`、`@locale/` は `locale/`、`@assets/` はルートの `assets/`）。別名は `alias.ts` と `tsconfig.json` の `paths` で揃える。

## ページと言語

- ページは言語ごとのパス（`/en/…`、`/ja/…`）に置く。言語のないパス（`/`、`/download` など）は、`src/server/localeRedirect.ts` が cookie（言語メニューで選んだ言語）か Accept-Language で選んだ言語へ 308 で転送する。
- 文言は `locale/{en,ja}.json` に置き、`useT(名前空間)` で引く。英語と日本語のキーと変数が揃っていること、英語がイギリス英語の綴りであること、日本語の和欧文の間隔はテストが確かめる。文言はアプリの翻訳のガイドライン（`apps/nyaw/crates/i18n/locales/*/GUIDELINES.md`）に従う。
- ページのタイトルは「（ページ名） / nyaw」。`head` は `src/seo.ts` の `seo` で、canonical、言語ごとの hreflang、OGP を出す。robots.txt と sitemap.xml は `src/server/seoFiles.ts` が返す。

## 見た目

- 基調はアプリのトークンの無彩色、アクセントは Catppuccin（ライトは Latte、ダークはロゴと同じ Frappé。`ctp-*` の色）。
- テーマは保存したもの（`localStorage`）か OS の設定に従う。`src/theme.ts` の `themeScript` が描画の前に `<html data-theme>` を付ける。切り替えは画面全体のクロスフェード（View Transitions API）で見せ、要素ごとの色のトランジションは使わない（継承した色の補間が毎フレーム作り直され、文字やアイコンがチカチカする）。
- 角はスーパーエリプス（`corner-shape`）。対応するブラウザでは、トークンの角丸に `--corner-scale` を掛けて広げる。中に項目を並べるパネルは、外側の半径を「中の半径＋余白」にして角を同心にする。
- 下位のページは、ページ名を大きく置く上部（`PageTitle`。英語の名前を D-DIN で表示する）と、一段沈んだ色の本文（`PageSection`）に分ける。
- ページは帯（外枠）で囲み、その内側に本文のカードを置く。ヘッダーは上辺の帯に、フッターはカードの下の帯に載る。帯の寸法（`--frame-*`。左右の帯は md 以上だけ）、帯の色（`frame`。ライトはアプリの groove より二段明るい）、重なり順（`--layer-*`）は `src/styles.css` の `:root` にまとめ、ほかの場所に数値を書かない。帯の内側に置くものの幅は `frame-x` でそろえる。
- 区画の中に置く面は `SURFACE`（`src/ui/variants.ts`）から選ぶ。同じクラスの組み合わせが 2 か所以上に出てきたら、`variants.ts` に名前を付けて足す。
- UI の状態の切り替え（ポップアップの出入り、パネルの切り替え）の時間は、トークンの時間（`src/motion/duration.ts` の `duration`）を使う。ページの演出（入場、スクロールに連動する動き）の時間は、それぞれの動きが持つ。

## アプリのキャプチャ

ランディングページと機能紹介の絵は、アプリのパネルのキャプチャ（`public/scene/<テーマ>-<言語>/<場面>/`）を CSS の 3D 変形で重ねたもの（`src/composite/Composite.tsx`）。キャプチャはアプリの GPU テストが書き出す。アプリの見た目が変わったら、`apps/nyaw/` で次を実行して作り直す（Windows と GPU が要る）。

```sh
cargo test -p ui --test gpu -- --ignored site
```

OGP の画像（`public/og-<言語>.png`）は `pnpm og` で作り直す（キャプチャか文言を変えたとき）。

## コマンド

このディレクトリで実行する（ルートからは `pnpm --filter web <名前>`）。

| コマンド                                     | 内容                                         |
| -------------------------------------------- | -------------------------------------------- |
| `pnpm dev`                                   | 開発サーバー                                 |
| `pnpm build` / `pnpm preview`                | 本番ビルドと、その手元での確認               |
| `pnpm fmt` / `pnpm fmt:check`                | 整形と、その確認                             |
| `pnpm lint` / `pnpm typecheck` / `pnpm test` | 静的解析、型検査、単体テスト                 |
| `pnpm e2e`                                   | E2E テスト（ビルドしたサイトを配信して実行） |
| `pnpm og`                                    | OGP の画像の書き出し                         |
| `pnpm run deploy`                            | Cloudflare Workers へのデプロイ              |

E2E テストを初めて実行するときは、`pnpm exec playwright install chromium webkit` でブラウザを取得する。

CI（`.github/workflows/web.yml`）は整形の確認、静的解析、型検査、単体テスト、ビルド、E2E テストを、それぞれ別のジョブで並列に実行する。実行するのは、ドラフトでない PR の更新と `main` への push だけである。

## デザイントークン

`scripts/tokens.mjs` がアプリの `tokens.json` から `src/tokens.css` を生成する（`dev`・`build`・`typecheck` が実行する。コミットしない）。

- 色は役割の名前で Tailwind に登録し（`bg-card`、`text-neutral-text` など）、Tailwind の既定の色は使わない。ライトとダークは `light-dark()` で、`color-scheme`（テーマ）に従う。
- 角丸と時間も共有する。余白は Tailwind の既定（4px の倍数）を使う。文字の大きさなど、アプリの画面向けの値は共有しない。
- コントラストの検査はアプリのビルドが `tokens.json` に定義された組み合わせに対して行う。ウェブサイト独自の組み合わせは検査されない。
- ウェブサイトだけで変える値（帯の色、ライトの本文の色など）は、`tokens.json` を変えず、`src/styles.css` の `:root` で上書きする。片方のテーマだけ変えるときは、`@variant light` か `@variant dark` の中に書く。上書きした値もコントラストの検査を受けない。
- 参照の解決規則は `apps/nyaw/crates/design/build.rs` と重複する。解決できない参照があると生成は失敗する。

`src/routeTree.gen.ts` は TanStack Router が生成する。手で編集せず、コミットする。
