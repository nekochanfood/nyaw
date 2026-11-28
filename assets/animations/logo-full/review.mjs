// レビュー用のページを配信する。JSON を fetch で読むので、ファイルを直接開くのではなくサーバー越しに見る。
// 実行: node assets/animations/logo-full/review.mjs [ポート]
// 開く: http://localhost:4173/animations/logo-full/review.html
// assets を丸ごと配信するのは、比較用に assets/logos の静止画も読むため。

import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const PORT = Number(process.argv[2] ?? 4173);
const TYPES = { ".html": "text/html", ".json": "application/json", ".svg": "image/svg+xml", ".js": "text/javascript" };

createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname));
  try {
    const body = await readFile(join(ROOT, path));
    res.writeHead(200, { "content-type": TYPES[extname(path)] ?? "application/octet-stream", "cache-control": "no-store" });
    res.end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(PORT, "127.0.0.1", () => console.log(`http://localhost:${PORT}/animations/logo-full/review.html`));
