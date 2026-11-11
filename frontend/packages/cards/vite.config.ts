import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import pkg from "./package.json" with { type: "json" };

// Library build: ES modules with the heavy renderers in lazy chunks, the
// dependencies left for the consumer's bundler, and one stylesheet.
const external = Object.keys(pkg.dependencies);

export default defineConfig({
  build: {
    target: "es2022",
    outDir: "dist",
    lib: {
      entry: fileURLToPath(new URL("./src/index.ts", import.meta.url)),
      formats: ["es"],
      fileName: "index",
      cssFileName: "styles",
    },
    rollupOptions: {
      external: (id) => external.some((dep) => id === dep || id.startsWith(`${dep}/`)),
    },
  },
});
