// OGP の画像（public/og-<言語>.png、1200×630）を書き出す。パネルのキャプチャを重ねた絵とキャッチコピーを
// Chromium で描いて撮る。キャプチャか文言を変えたら `pnpm og` で作り直す。
import { readFileSync } from "node:fs";
import { chromium } from "@playwright/test";

const file = (path) => new URL(path, import.meta.url);
const dataUrl = (path, type) =>
  `data:${type};base64,${readFileSync(file(path)).toString("base64")}`;
const panel = (locale, moment, role) =>
  dataUrl(`../public/scene/light-${locale}/${moment}/${role}.png`, "image/png");

function html(locale) {
  const messages = JSON.parse(readFileSync(file(`../locale/${locale}.json`), "utf8"));
  const logo = dataUrl("../../../assets/logos/Logo_Full_Light.svg", "image/svg+xml");
  return `<!doctype html><html lang="${locale}"><head><meta charset="utf-8"><style>
    * { margin: 0; box-sizing: border-box; }
    body { width: 1200px; height: 630px; overflow: hidden; font-family: "Gen Interface JP", Inter, "Noto Sans JP", "Yu Gothic UI", "Hiragino Sans", sans-serif;
      background: linear-gradient(135deg, #e7e9ff, #fbe4f3 55%, #ffe9dc); color: #3d3d3d; word-break: auto-phrase; }
    .text { position: absolute; left: 72px; top: 72px; width: 480px; display: flex; flex-direction: column; gap: 24px; }
    .logo { height: 72px; width: auto; align-self: flex-start; }
    h1 { font-size: 56px; line-height: 1.15; font-weight: 700; letter-spacing: -0.02em; }
    p { font-size: 22px; color: #666; }
    .stage { position: absolute; right: 10px; top: 90px; width: 640px; height: 460px; perspective: 1600px; }
    .scene { position: absolute; inset: 0; transform-style: preserve-3d; transform: rotateX(8deg) rotateY(-14deg); }
    .scene img { position: absolute; height: auto; }
  </style></head><body>
    <div class="text">
      <img class="logo" src="${logo}" alt="">
      <h1>${messages.hero.title}</h1>
      <p>${messages.hero.eyebrow}</p>
    </div>
    <div class="stage"><div class="scene">
      <img src="${panel(locale, "mirroring", "mirroring")}" style="left: 4%; top: 2%; width: 86%">
      <img src="${panel(locale, "mirroring", "dashboard")}" style="left: -12%; top: 66%; width: 122%; transform: translateZ(80px) rotateX(16deg)">
      <img src="${panel(locale, "mirroring", "wrist")}" style="left: 66%; top: 52%; width: 28%; transform: translateZ(160px)">
    </div></div>
  </body></html>`;
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 630 } });
for (const locale of ["en", "ja"]) {
  await page.setContent(html(locale), { waitUntil: "load" });
  await page.screenshot({
    path: file(`../public/og-${locale}.png`).pathname.replace(/^\/(\w:)/, "$1"),
  });
  console.log(`og-${locale}.png`);
}
await browser.close();
