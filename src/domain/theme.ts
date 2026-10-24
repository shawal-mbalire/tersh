// Theme colours as CSS: the variables cards, toasts and plugins style themselves with.

import { withSymbols } from "./fonts";
import { parseHex } from "./palette";
import type { Config } from "./types";

/** Relative luminance of a `#rrggbb` colour, 0 (black) to 1 (white). */
export function luminance(color: string): number {
  const [r, g, b] = (parseHex(color) ?? [0, 0, 0]).map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export const isLight = (color: string) => luminance(color) > 0.4;

/** CSS custom properties for a config. Cards, toasts and plugins style themselves with these. */
export function cssVariables(config: Config): Record<string, string> {
  const { theme, window, font } = config;
  const [r, g, b] = parseHex(theme.background) ?? [0, 0, 0];
  const vars: Record<string, string> = {
    "--bg": `rgba(${r}, ${g}, ${b}, ${Math.min(1, Math.max(0, window.opacity))})`,
    "--bg-solid": theme.background,
    "--fg": theme.foreground,
    "--cursor": theme.cursor,
    "--selection": theme.selection,
    "--accent": theme.accent,
    "--padding": `${window.padding}px`,
    "--font-family": withSymbols(font.family),
    "--font-size": `${font.size}px`,
  };
  theme.palette.forEach((color, i) => (vars[`--ansi-${i}`] = color));
  return vars;
}
