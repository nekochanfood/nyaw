import { defineConfig } from "vitest/config";

import { alias } from "./alias.ts";

// vite.config.ts（Cloudflare と TanStack Start のプラグイン）はテストに使わない
export default defineConfig({
  resolve: { alias },
  test: { include: ["scripts/**/*.test.mjs", "src/**/*.test.{ts,tsx}"] },
});
