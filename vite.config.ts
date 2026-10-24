import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
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
