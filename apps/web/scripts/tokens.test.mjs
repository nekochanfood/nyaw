import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { tokensCss } from "./tokens.mjs";

const tokens = JSON.parse(
  readFileSync(new URL("../../nyaw/crates/design/tokens.json", import.meta.url), "utf8"),
);

test("アプリのトークンから角丸と時間を書き出し、色は書き出さない", () => {
  const css = tokensCss(tokens);
  expect(css).toContain("--radius-md: calc(8px * var(--corner-scale, 1));");
  expect(css).toContain("--radius-full: 9999px;");
  expect(css).toContain(`--duration-normal: ${tokens["duration-ms"].normal}ms;`);
  expect(css).not.toContain("--color-");
});
