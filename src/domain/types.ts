// Shapes shared with the Rust side (crates/tersh-domain and crates/tersh-app).

export interface Run {
  /** Starting column. */
  c: number;
  /** Cells covered. If `t` has exactly `n` code points each sits in its own cell, else `t` is one cluster. */
  n: number;
  t: string;
  fg: number;
  bg: number;
  /** alacritty cell flags, see `CellFlag`. */
  f: number;
}

export interface Row {
  y: number;
  runs: Run[];
}

export type CursorShape = "block" | "underline" | "beam" | "hollow" | "hidden";

export interface Frame {
  cols: number;
  rows: number;
  full: boolean;
  lines: Row[];
  cursor: { x: number; y: number; shape: CursorShape; blink: boolean };
  mode: number;
  alt: boolean;
  display_offset: number;
  history: number;
  scroll_base: number;
}

export type TermEvent =
  | { type: "title"; title: string }
  | { type: "reset_title" }
  | { type: "bell" }
  | { type: "clipboard"; text: string }
  | { type: "rich_error"; error: string }
  | { type: "exit"; code: number | null };

/** Cell size in CSS pixels. */
export interface Metrics {
  /** CSS pixels. */
  cellWidth: number;
  cellHeight: number;
  baseline: number;
}

export interface SessionInfo {
  version: string;
  cols: number;
  rows: number;
  cell_width: number;
  cell_height: number;
}

export interface Theme {
  name: string;
  foreground: string;
  background: string;
  cursor: string;
  selection: string;
  accent: string;
  palette: string[];
}

/** An action name, or text to type into the shell. */
export type Binding = string | { send: string };

export interface Config {
  shell: string | null;
  args: string[];
  env: Record<string, string>;
  scrollback: number;
  font: { family: string; size: number; line_height: number; bold_is_bright: boolean };
  window: { padding: number; opacity: number; blur: boolean; width: number; height: number };
  cursor: { shape: "block" | "beam" | "underline"; blink: boolean };
  theme: Theme;
  keybindings: Record<string, Binding>;
  rich: { enabled: boolean; osc: boolean; html_network: boolean };
  plugins: { disabled: string[] };
  update: { auto_restart: boolean };
  /** Problems found while loading the config file. */
  warnings: string[];
}

/** Everything the page gets from the user's config directory. */
export interface Settings {
  config: Config;
  /** Contents of `style.css`. */
  style: string | null;
  config_path: string;
  /** Theme names usable in `theme = "..."`. */
  themes: string[];
  /** Changes whenever a plugin file changes. */
  plugins_revision: string;
  /** Changes whenever fonts are installed or removed. */
  fonts_revision: string;
}

/** alacritty_terminal `Flags` bits. */
export const CellFlag = {
  INVERSE: 1 << 0,
  BOLD: 1 << 1,
  ITALIC: 1 << 2,
  UNDERLINE: 1 << 3,
  WIDE_CHAR: 1 << 5,
  DIM: 1 << 7,
  HIDDEN: 1 << 8,
  STRIKEOUT: 1 << 9,
  DOUBLE_UNDERLINE: 1 << 11,
  UNDERCURL: 1 << 12,
  DOTTED_UNDERLINE: 1 << 13,
  DASHED_UNDERLINE: 1 << 14,
} as const;

/** alacritty_terminal `TermMode` bits. */
export const Mode = {
  SHOW_CURSOR: 1 << 0,
  APP_CURSOR: 1 << 1,
  APP_KEYPAD: 1 << 2,
  MOUSE_REPORT_CLICK: 1 << 3,
  BRACKETED_PASTE: 1 << 4,
  SGR_MOUSE: 1 << 5,
  MOUSE_MOTION: 1 << 6,
  FOCUS_IN_OUT: 1 << 11,
  ALT_SCREEN: 1 << 12,
  MOUSE_DRAG: 1 << 13,
  ALTERNATE_SCROLL: 1 << 15,
  MOUSE_MODE: (1 << 3) | (1 << 6) | (1 << 13),
} as const;

/** Encoded colours with this bit carry literal 0xRRGGBB; otherwise they are palette indices. */
export const RGB_FLAG = 1 << 24;
