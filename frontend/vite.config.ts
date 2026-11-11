import angular from "@analogjs/vite-plugin-angular";
import { defineConfig } from "vite";
import { aliases } from "./aliases";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
  // Angular hosts the page shell (src/adapters/angular); compiled ahead of time.
  plugins: [angular({ tsconfig: "tsconfig.app.json", jit: false })],
  resolve: {
    // Named imports instead of relative paths (see aliases.ts); the card
    // renderers in packages/cards are built from source.
    alias: aliases,
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/backend/**", "**/target/**"] },
  },
  build: {
    target: "es2022",
    // Heavy renderers (Plotly, KaTeX, highlight.js) are split into lazy chunks.
    chunkSizeWarningLimit: 5000,
  },
});
