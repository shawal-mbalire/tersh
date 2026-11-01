// Keeps the hexagonal layering honest: domain code is pure, use cases only
// talk to ports, the page (adapters/dom) never touches the host's API, and
// only adapters touch the DOM or Tauri.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const src = join(__dirname, "..");

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "__tests__" ? [] : files(path);
    return path.endsWith(".ts") ? [path] : [];
  });
}

/** Layer of an import, resolved from the importing file. */
function importsOf(file: string): string[] {
  const text = readFileSync(file, "utf8");
  return [...text.matchAll(/(?:from\s+|import\s*\(?\s*)["']([^"']+)["']/g)].map(([, spec]) => {
    if (!spec.startsWith(".")) return spec;
    return relative(src, join(file, "..", spec));
  });
}

const rules: Record<string, (target: string) => boolean> = {
  // Pure logic: nothing but other domain modules.
  domain: (t) => t.startsWith("domain"),
  // Interfaces over the domain.
  ports: (t) => t.startsWith("domain") || t.startsWith("ports"),
  // Use cases: the domain and the ports they drive, nothing concrete.
  app: (t) => t.startsWith("domain") || t.startsWith("ports") || t.startsWith("app"),
  // The page: drives the use cases and implements the screen port; never a host API.
  "adapters/dom": (t) => !t.startsWith("adapters/tauri") && !t.startsWith("@tauri-apps"),
  // The host: Tauri, never the page.
  "adapters/tauri": (t) => !t.startsWith("adapters/dom"),
};

/** Browser globals only adapters may touch. */
const DOM_GLOBALS = /(?<![.\w])(document|window|navigator|localStorage)\.|\b(requestAnimationFrame|HTMLElement|ResizeObserver)\b/;

/** Code without comments, so prose can mention the window or the document. */
function code(file: string): string {
  return readFileSync(file, "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/(^|[^:])\/\/.*$/gm, "$1");
}

describe("architecture", () => {
  for (const [layer, allowed] of Object.entries(rules)) {
    it(`${layer}/ only imports what it may`, () => {
      const violations = files(join(src, layer)).flatMap((file) =>
        importsOf(file)
          .filter((target) => !allowed(target))
          .map((target) => `${relative(src, file)} -> ${target}`),
      );
      expect(violations).toEqual([]);
    });
  }

  for (const layer of ["domain", "ports", "app"]) {
    it(`${layer}/ doesn't touch the DOM`, () => {
      const violations = files(join(src, layer)).filter((file) => DOM_GLOBALS.test(code(file)));
      expect(violations.map((file) => relative(src, file))).toEqual([]);
    });
  }
});
