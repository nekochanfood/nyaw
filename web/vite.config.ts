// ページごとに入口の HTML を持つマルチページ構成。出力は `<ページ名>/index.html` と共有の `assets/`。
// UI 層は `nyaw://localhost/<ページ名>/index.html` で配信するので、相対パス（base: "./"）で参照させる。
// 出力先は crates/webpages の build.rs が NYAW_WEB_OUT で OUT_DIR を指す（無ければ dist）。
import { resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

/** ページ名（= ディレクトリ名）。Rust 側の `PanelSpec::web` はこの名前で開く。 */
const pages = ["testing", "testing-transparent"];

export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss()],
  build: {
    outDir: process.env.NYAW_WEB_OUT ?? "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: Object.fromEntries(
        pages.map((p) => [p, resolve(import.meta.dirname, p, "index.html")]),
      ),
    },
  },
});
