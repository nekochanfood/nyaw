import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  use: { baseURL: "http://127.0.0.1:4173", viewport: { width: 800, height: 600 } },
  webServer: {
    cwd: import.meta.dirname,
    command: "npm exec vite preview -- --host 127.0.0.1 --port 4173 --strictPort",
    url: "http://127.0.0.1:4173/testing/index.html",
  },
});
