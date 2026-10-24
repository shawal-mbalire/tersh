// Keeps the hexagonal layering honest: domain code is pure, the UI only talks
// to the host through the Backend port, and only adapters touch Tauri.

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
  // DOM code: domain, ports and rendering libraries, never a host API.
  ui: (t) => !t.startsWith("adapters") && !t.startsWith("@tauri-apps"),
  adapters: () => true,
};

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
});
