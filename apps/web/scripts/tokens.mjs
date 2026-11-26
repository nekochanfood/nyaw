// アプリのデザイントークン（tokens.json）から src/tokens.css を生成する。
// 参照の解決は apps/nyaw/crates/design/build.rs と同じ規則。コントラストの検査はそちらが行う。
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export function tokensCss({ scales, static: statics, steps, aliases, mutable, radius, ...rest }) {
  const resolve = (theme, ref) => {
    if (Object.hasOwn(statics, ref)) return statics[ref];
    const [, name, step] = /^(.+)-(\d+)$/.exec(ref) ?? [];
    const color = scales[aliases[name] ?? name]?.[theme]?.[Number(step) - 1];
    if (!color) throw new Error(`未知の色の参照: ${ref}（${theme}）`);
    return color;
  };
  const roles = (theme) => {
    const map = new Map();
    for (const alias of Object.keys(aliases)) {
      steps.forEach((step, i) => map.set(`${alias}-${step}`, resolve(theme, `${alias}-${i + 1}`)));
    }
    for (const [role, ref] of Object.entries(mutable[theme])) map.set(role, resolve(theme, ref));
    return map;
  };
  const light = roles("light");
  const dark = roles("dark");
  const colors = [...light.keys()].map((role) => {
    if (!dark.has(role)) throw new Error(`ダークテーマに役割がない: ${role}`);
    return `  --${role}: light-dark(${light.get(role)}, ${dark.get(role)});`;
  });
  const durations = Object.entries(rest["duration-ms"]).map(
    ([name, ms]) => `  --duration-${name}: ${ms}ms;`,
  );
  return [
    "/* scripts/tokens.mjs が生成する。編集しない。 */",
    ":root {",
    "  color-scheme: light dark;",
    ...colors,
    ...durations,
    "}",
    "",
    "/* Tailwind の色はトークンの役割だけにする */",
    "@theme inline {",
    "  --color-*: initial;",
    ...[...light.keys()].map((role) => `  --color-${role}: var(--${role});`),
    ...Object.entries(radius).map(([name, px]) => `  --radius-${name}: ${px}px;`),
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
