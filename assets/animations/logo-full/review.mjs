// レビュー用のページを配信する。JSON を fetch で読むので、ファイルを直接開くのではなくサーバー越しに見る。
// 実行: node assets/animations/logo-full/review.mjs [ポート]
// 開く: http://localhost:4173/animations/logo-full/review.html
// assets を丸ごと配信するのは、比較用に assets/logos の静止画も読むため。

import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const TYPES = { ".html": "text/html", ".json": "application/json", ".svg": "image/svg+xml", ".js": "text/javascript" };

// assets を配信するサーバーを立てる。handle(req, res) が true を返したリクエストは、配信せずにそちらに任せる。
// port に 0 を渡すと空いているポートを使う。立ち上がったら、そのサーバーを返す
export function serve(port, handle = () => false) {
  const server = createServer(async (req, res) => {
    if (await handle(req, res)) return;
    const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname));
    const file = join(ROOT, path);
    // assets の外は配信しない
    if (!file.startsWith(ROOT)) return void res.writeHead(404).end();
    try {
      const body = await readFile(file);
      res.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream", "cache-control": "no-store" });
      res.end(body);
    } catch {
      res.writeHead(404).end();
    }
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const server = await serve(Number(process.argv[2] ?? 4173));
  console.log(`http://localhost:${server.address().port}/animations/logo-full/review.html`);
}
