import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";

export default defineConfig({
  testDir: "./tests",
  outputDir: "./test-results",
  use: { baseURL: "http://127.0.0.1:4173", viewport: { width: 800, height: 600 } },
  webServer: {
    command: "npx vite preview --config web/vite.config.ts --host 127.0.0.1 --port 4173 --strictPort",
    cwd: fileURLToPath(new URL("..", import.meta.url)),
    url: "http://127.0.0.1:4173",
  },
});
