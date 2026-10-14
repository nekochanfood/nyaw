import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  build: {
    // 全画面をクライアントで描くので、依存の "use client" 指定を保持する必要は無い。
    rolldownOptions: { checks: { moduleLevelDirective: false } },
  },
});
