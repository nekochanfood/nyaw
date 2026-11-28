// イントロを MP4 かアニメーション WebP に書き出す。ヘッドレスの Chrome で export.html を開いて 1 フレームずつ PNG にし、ffmpeg でまとめる。
// 実行: node assets/animations/logo-full/export.mjs [light|dark] [--webp] [--scale n] [--bg #rrggbb]
// テーマを省くと両方を書き出す。出力は assets/animations/logo-full/out/logo-full-<テーマ>.mp4。
// --webp は透過の logo-full-<テーマ>.webp を、このディレクトリに書き出す（1 回だけ再生し、完成形で止まる）。README は light を使う。
// 倍率の既定は MP4 が 2 倍、WebP は容量を抑えるため 1.5 倍
// Chrome と ffmpeg が要る。Chrome の場所は環境変数 CHROME で変えられる。
// MP4 は透明を持てないので、背景色を敷く（既定はレビューのページと同じ色）

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { serve } from "./review.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BACKGROUNDS = { light: "#eff1f5", dark: "#303446" };
const FPS = 60;
const TIMEOUT_MS = 5 * 60 * 1000;

const CHROME_CANDIDATES = [
  process.env.CHROME,
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].filter(Boolean);

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: { scale: { type: "string" }, bg: { type: "string" }, webp: { type: "boolean", default: false } },
});
const themes = positionals.length > 0 ? positionals : ["light", "dark"];
const scale = Number(values.scale ?? (values.webp ? 1.5 : 2));
for (const theme of themes) if (!(theme in BACKGROUNDS)) throw new Error(`テーマは light か dark: ${theme}`);
if (!(scale > 0)) throw new Error(`--scale は正の数: ${values.scale}`);

const chrome = CHROME_CANDIDATES.find((path) => existsSync(path));
if (!chrome) throw new Error("Chrome が見つからない。環境変数 CHROME に場所を指定する");

// 子プロセスを動かし、終わるのを待つ。失敗したら、標準エラーの末尾を付けて例外にする
function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ["ignore", "ignore", "pipe"] });
    let stderr = "";
    child.stderr.on("data", (chunk) => (stderr = (stderr + chunk).slice(-4000)));
    child.on("error", reject);
    child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error(`${command} が ${code} で終わった\n${stderr}`))));
  });
}

// 1 つのテーマを、frames に PNG として書き出す。ページが全フレームを送り終えたら返る
async function renderFrames(theme, background, frames) {
  let finish;
  const done = new Promise((resolve) => (finish = resolve));
  const server = await serve(0, async (req, res) => {
    const url = new URL(req.url, "http://x");
    if (req.method !== "POST" || !url.pathname.startsWith("/export/")) return false;
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const frame = url.pathname.match(/^\/export\/frame\/(\d+)$/)?.[1];
    if (frame !== undefined) await writeFile(join(frames, `${frame.padStart(4, "0")}.png`), Buffer.concat(chunks));
    res.writeHead(204).end();
    if (url.pathname === "/export/done") finish();
    return true;
  });
  const query = new URLSearchParams({ theme, scale: String(scale), bg: background });
  const page = `http://127.0.0.1:${server.address().port}/animations/logo-full/export.html?${query}`;
  const browser = spawn(chrome, ["--headless=new", "--disable-gpu", `--user-data-dir=${join(frames, "..", "chrome")}`, page], {
    stdio: "ignore",
  });
  const timeout = new Promise((_, reject) => setTimeout(() => reject(new Error("書き出しが時間内に終わらなかった")), TIMEOUT_MS).unref());
  try {
    await Promise.race([done, timeout]);
  } finally {
    browser.kill();
    server.close();
  }
}

await mkdir(join(HERE, "out"), { recursive: true });
for (const theme of themes) {
  const work = await mkdtemp(join(tmpdir(), "logo-full-"));
  const frames = join(work, "frames");
  await mkdir(frames);
  const background = values.bg ?? (values.webp ? "" : BACKGROUNDS[theme]);
  try {
    console.log(`${theme}: フレームを描いている`);
    await renderFrames(theme, background, frames);
    const out = values.webp ? join(HERE, `logo-full-${theme}.webp`) : join(HERE, "out", `logo-full-${theme}.mp4`);
    const encode = values.webp
      ? ["-c:v", "libwebp_anim", "-pix_fmt", "yuva420p", "-quality", "75", "-loop", "1"]
      : [
          // H.264 の 4:2:0 は縦横が偶数でないと書けないので、奇数なら背景色で 1 ピクセル足す
          "-vf", `pad=ceil(iw/2)*2:ceil(ih/2)*2:color=${background}`,
          "-c:v", "libx264",
          "-pix_fmt", "yuv420p",
          "-crf", "16",
          "-movflags", "+faststart",
        ];
    await run("ffmpeg", ["-y", "-framerate", String(FPS), "-i", join(frames, "%04d.png"), ...encode, out]);
    console.log(`wrote ${out}`);
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}
