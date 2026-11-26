import { defineConfig } from "vitest/config";

// vite.config.ts（Cloudflare と TanStack Start のプラグイン）はテストに使わない
export default defineConfig({
  test: { include: ["scripts/**/*.test.mjs", "src/**/*.test.{ts,tsx}"] },
});
