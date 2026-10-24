// Key combos and the keybinding table: which key runs which action.

import type { Binding } from "./types";

/** Normalise `"Shift+Ctrl+K"` → `"ctrl+shift+k"`. A literal plus key may be written `plus` or `+`. */
export function normalizeCombo(combo: string): string {
  const parts = combo
    .trim()
    .replace(/(^|\+)\+$/, "$1plus")
    .toLowerCase()
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean);
  const key = parts.pop() ?? "";
  const alias: Record<string, string> = {
    cmd: "meta",
    super: "meta",
    win: "meta",
    control: "ctrl",
    option: "alt",
    esc: "escape",
    space: " ",
    plus: "+",
  };
  const mods = new Set(parts.map((p) => alias[p] ?? p));
  const order = ["ctrl", "alt", "shift", "meta"].filter((m) => mods.has(m));
  return [...order, alias[key] ?? key].join("+");
}

/** The combo string for a keyboard event, comparable with `normalizeCombo` output. */
export function comboFromEvent(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string {
  const mods = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "meta"].filter(Boolean);
  return [...mods, e.key.toLowerCase()].join("+");
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
    [`${primary}+f5`]: "restart",
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
