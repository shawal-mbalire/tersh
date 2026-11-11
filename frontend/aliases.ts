// Import aliases, shared by the app's and the cards example's Vite configs and
// mirrored in tsconfig.json `paths`. Code imports by name (`@domain/input`,
// `@tersh/cards`) instead of climbing directories; folders are named here,
// from this file's location, and nowhere else.

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** The frontend folder (where this file is). */
export const frontend = dirname(fileURLToPath(import.meta.url));
/** The repository root, the frontend folder's parent. */
export const repo = dirname(frontend);

const src = join(frontend, "src");
const cards = join(frontend, "packages", "cards", "src");

export const aliases: Record<string, string> = {
  // The page's layers.
  "@domain": join(src, "domain"),
  "@ports": join(src, "ports"),
  "@app": join(src, "app"),
  "@adapters": join(src, "adapters"),
  // The card renderers, built from source: the public entry, its styles, and
  // (for tests) its modules.
  "@tersh/cards/styles.css": join(cards, "styles.css"),
  "@tersh/cards": join(cards, "index.ts"),
  "@cards": cards,
  // The page's own manifest (its version), and the repository's examples.
  "@frontend/package.json": join(frontend, "package.json"),
  "@examples": join(repo, "examples"),
};
