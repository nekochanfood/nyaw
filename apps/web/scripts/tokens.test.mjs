import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { tokensCss } from "./tokens.mjs";

const tokens = JSON.parse(
  readFileSync(new URL("../../nyaw/crates/design/tokens.json", import.meta.url), "utf8"),
);

test("アプリのトークンをライトとダークの役割に解決する", () => {
  const css = tokensCss(tokens);
  const { gray } = tokens.scales;
  expect(css).toContain(`--background: light-dark(${gray.light[2]}, ${gray.dark[0]});`);
  expect(css).toContain(`--neutral-text: light-dark(${gray.light[11]}, ${gray.dark[11]});`);
  expect(css).toContain("--color-background: var(--background);");
  expect(css).toContain("--radius-md: calc(8px * var(--corner-scale, 1));");
  expect(css).toContain("--radius-full: 9999px;");
});

test("未知の参照は失敗する", () => {
  const broken = structuredClone(tokens);
  broken.mutable.dark.card = "neutral-13";
  expect(() => tokensCss(broken)).toThrow("neutral-13");
});
