// Keeps the hexagonal layering honest: domain code is pure, use cases only
// talk to ports, the page (adapters/dom) never touches the host's API or the
// framework, the Angular shell never names a host, and only adapters touch the
// DOM, Angular or Tauri.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { describe, expect, it } from "bun:test";

const src = dirname(__dirname);

/** Aliases naming the page's layers (see aliases.ts), as folders under src/. */
const LAYER_ALIASES = ["@domain/", "@ports/", "@app/", "@adapters/"];

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "__tests__" ? [] : files(path);
    return path.endsWith(".ts") ? [path] : [];
  });
}

/** Layer of an import: aliases and same-folder paths become paths under src/. */
function importsOf(file: string): string[] {
  const text = readFileSync(file, "utf8");
  return [...text.matchAll(/(?:from\s+|import\s*\(?\s*)["']([^"']+)["']/g)].map(([, spec]) => {
    const layer = LAYER_ALIASES.find((alias) => spec.startsWith(alias));
    if (layer) return spec.slice(1);
    if (spec.startsWith("./")) return relative(src, join(dirname(file), spec));
    return spec;
  });
}

const rules: Record<string, (target: string) => boolean> = {
  // Pure logic: nothing but other domain modules.
  domain: (t) => t.startsWith("domain"),
  // Interfaces over the domain.
  ports: (t) => t.startsWith("domain") || t.startsWith("ports"),
  // Use cases: the domain and the ports they drive, nothing concrete.
  app: (t) => t.startsWith("domain") || t.startsWith("ports") || t.startsWith("app"),
  // The page: drives the use cases and implements the screen port; never a
  // host API, and framework free, so another shell could host it.
  "adapters/dom": (t) =>
    !t.startsWith("adapters/tauri") && !t.startsWith("@tauri-apps") && !t.startsWith("adapters/angular") && !t.startsWith("@angular"),
  // The host: Tauri, never the page.
  "adapters/tauri": (t) => !t.startsWith("adapters/dom") && !t.startsWith("adapters/angular") && !t.startsWith("@angular"),
  // The Angular shell: hosts the page and receives its adapters through DI;
  // only the composition root (providers.ts) names a concrete host.
  "adapters/angular": (t) => !t.startsWith("adapters/tauri") && !t.startsWith("@tauri-apps"),
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
  it("imports never climb directories (use an alias from aliases.ts)", () => {
    const frontend = dirname(src);
    const all = [src, join(frontend, "packages", "cards")].flatMap((dir) =>
      readdirSync(dir, { recursive: true, encoding: "utf8" })
        .filter((name) => /\.(ts|json)$/.test(name) && !name.includes("node_modules") && !name.includes("dist"))
        .map((name) => join(dir, name)),
    );
    const climbing = all.filter((file) => /["']\.\.\//.test(readFileSync(file, "utf8")));
    expect(climbing.map((file) => relative(frontend, file))).toEqual([]);
  });

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
