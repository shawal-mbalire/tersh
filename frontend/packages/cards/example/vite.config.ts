import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// Serves the example page; it also reads the demo notebook from examples/demos.
// Folders are found from this file's parents: example → cards → packages →
// frontend → the repository.
const example = dirname(fileURLToPath(import.meta.url));
const cards = dirname(example);
const repo = dirname(dirname(dirname(cards)));

export default defineConfig({
  resolve: {
    alias: {
      "@tersh/cards/styles.css": join(cards, "src", "styles.css"),
      "@tersh/cards": join(cards, "src", "index.ts"),
      "@examples": join(repo, "examples"),
    },
  },
  server: { fs: { allow: [repo] } },
  build: { target: "es2022" },
});
