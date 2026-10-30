import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
  resolve: {
    // The card renderers live in packages/cards; the app builds them from source.
    alias: {
      "@tersh/cards/styles.css": fileURLToPath(new URL("./packages/cards/src/styles.css", import.meta.url)),
      "@tersh/cards": fileURLToPath(new URL("./packages/cards/src/index.ts", import.meta.url)),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  build: {
    target: "es2022",
    // Heavy renderers (Plotly, KaTeX, highlight.js) are split into lazy chunks.
    chunkSizeWarningLimit: 5000,
  },
  test: {
    environment: "jsdom",
  },
});
