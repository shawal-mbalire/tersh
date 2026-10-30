import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// Serves the example page; it also reads the demo notebook from examples/demos.
export default defineConfig({
  server: { fs: { allow: [fileURLToPath(new URL("../../..", import.meta.url))] } },
  build: { target: "es2022" },
});
