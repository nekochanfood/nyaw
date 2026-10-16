// Phosphor Icons (@phosphor-icons/core, MIT) の SVG を、ウェイトごとの 1 ファイルにまとめ直す。
// 使い方: node icons/vendor.mjs <展開した @phosphor-icons/core のディレクトリ>
// 1 行 = `名前<TAB>d<TAB>d...`。`~` で始まる d は duotone の薄い面（opacity 0.2）。
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const src = process.argv[2];
for (const weight of ["thin", "light", "regular", "bold", "fill", "duotone"]) {
  const dir = join(src, "assets", weight);
  const lines = readdirSync(dir)
    .filter((f) => f.endsWith(".svg"))
    .sort()
    .map((f) => {
      const name = f.slice(0, -4).replace(new RegExp(`-${weight}$`), "");
      const paths = [...readFileSync(join(dir, f), "utf8").matchAll(/<path ([^>]*)\/>/g)].map(
        ([, attrs]) => {
          const d = /d="([^"]*)"/.exec(attrs)[1];
          return /opacity=/.test(attrs) ? `~${d}` : d;
        },
      );
      return [name, ...paths].join("\t");
    });
  writeFileSync(new URL(`./${weight}.tsv`, import.meta.url), lines.join("\n") + "\n");
}
