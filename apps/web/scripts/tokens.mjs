// アプリのデザイントークン（tokens.json）から、角丸と時間を src/tokens.css に書き出す。
// 色はアプリと共有しない（ウェブサイトは Catppuccin の色を styles.css で持つ）。
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export function tokensCss({ radius, "duration-ms": durations }) {
  return [
    "/* scripts/tokens.mjs が生成する。編集しない。 */",
    ":root {",
    ...Object.entries(durations).map(([name, ms]) => `  --duration-${name}: ${ms}ms;`),
    "}",
    "",
    "@theme inline {",
    // 角の形がスーパーエリプスのときは、styles.css の --corner-scale で半径を広げる（full は真円のまま）
    ...Object.entries(radius).map(([name, px]) =>
      name === "full"
        ? `  --radius-${name}: ${px}px;`
        : `  --radius-${name}: calc(${px}px * var(--corner-scale, 1));`,
    ),
    "}",
    "",
  ].join("\n");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const source = new URL("../../nyaw/crates/design/tokens.json", import.meta.url);
  writeFileSync(
    new URL("../src/tokens.css", import.meta.url),
    tokensCss(JSON.parse(readFileSync(source, "utf8"))),
  );
}
