// Font lists: the bundled fonts, and adding the icon fallback to whatever the
// user configured so Nerd Font icons render with any text font.

/** Family name of the bundled Nerd Fonts symbols (icons only, see styles.css). */
export const SYMBOLS_FAMILY = "tersh Symbols";

/** Sample icons that make the browser load the symbols font (Powerline, Font Awesome, Material Design, Codicons). */
export const SYMBOLS_SAMPLE = "\u{f0001}";

const GENERIC = new Set([
  "serif",
  "sans-serif",
  "monospace",
  "cursive",
  "fantasy",
  "system-ui",
  "ui-monospace",
  "ui-sans-serif",
  "ui-serif",
  "emoji",
  "math",
  "fangsong",
]);

/** Split a CSS font-family list on commas outside quotes. */
export function splitFamilies(list: string): string[] {
  const out: string[] = [];
  let current = "";
  let quote: string | null = null;
  for (const ch of list) {
    if (quote) {
      if (ch === quote) quote = null;
      current += ch;
    } else if (ch === "'" || ch === '"') {
      quote = ch;
      current += ch;
    } else if (ch === ",") {
      out.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  if (current.trim()) out.push(current.trim());
  return out.filter(Boolean);
}

/** A family name without surrounding quotes. */
export const unquote = (family: string) => family.replace(/^(['"])(.*)\1$/, "$2");

/**
 * The configured family list with the bundled icon font added before the
 * first generic family (and `monospace` added if there is none).
 */
export function withSymbols(list: string): string {
  const families = splitFamilies(list);
  if (families.some((f) => unquote(f) === SYMBOLS_FAMILY)) return families.join(", ");
  const generic = families.findIndex((f) => GENERIC.has(f.toLowerCase()));
  const symbols = `'${SYMBOLS_FAMILY}'`;
  if (generic === -1) return [...families, symbols, "monospace"].join(", ");
  return [...families.slice(0, generic), symbols, ...families.slice(generic)].join(", ");
}
