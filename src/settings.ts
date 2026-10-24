// Applies user settings to the page: theme colours as CSS variables, the
// user's style.css, and the keybinding table.

import { normalizeCombo } from "./plugins/api";
import { parseHex } from "./term/palette";
import type { Binding, Config, Settings } from "./types";

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
    "--font-family": font.family,
    "--font-size": `${font.size}px`,
  };
  theme.palette.forEach((color, i) => (vars[`--ansi-${i}`] = color));
  return vars;
}

export function applySettings(settings: Settings, root: HTMLElement = document.documentElement) {
  const { config } = settings;
  for (const [name, value] of Object.entries(cssVariables(config))) root.style.setProperty(name, value);
  const light = isLight(config.theme.background);
  root.style.colorScheme = light ? "light" : "dark";
  root.dataset.theme = config.theme.name;
  root.dataset.themeKind = light ? "light" : "dark";

  // The user stylesheet goes last so it wins over built-in rules of equal specificity.
  let style = document.getElementById("user-style") as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = "user-style";
  }
  style.textContent = settings.style ?? "";
  document.head.append(style);
}

/** Default bindings; the user's `[keybindings]` are merged over these. */
export function defaultKeybindings(mac: boolean): Record<string, Binding> {
  const primary = mac ? "meta" : "ctrl+shift";
  const zoom = mac ? "meta" : "ctrl";
  return {
    [`${primary}+c`]: "copy",
    [`${primary}+v`]: "paste",
    [`${zoom}+=`]: "zoom_in",
    [`${zoom}+plus`]: "zoom_in",
    [`${zoom}+shift+plus`]: "zoom_in",
    [`${zoom}+-`]: "zoom_out",
    [`${zoom}+0`]: "zoom_reset",
    "shift+pageup": "scroll_page_up",
    "shift+pagedown": "scroll_page_down",
    [`${primary}+k`]: "clear_cards",
    [`${primary}+r`]: "reload_config",
    [`${zoom}+,`]: "open_config",
  };
}

/** Combined keybinding table keyed by normalised combo. `"none"` entries are dropped. */
export function resolveKeybindings(user: Record<string, Binding>, mac: boolean): Map<string, Binding> {
  const table = new Map<string, Binding>();
  for (const source of [defaultKeybindings(mac), user]) {
    for (const [combo, binding] of Object.entries(source)) table.set(normalizeCombo(combo), binding);
  }
  for (const [combo, binding] of table) if (binding === "none") table.delete(combo);
  return table;
}
