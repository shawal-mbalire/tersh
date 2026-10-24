import { Mode } from "../types";

export interface KeyLike {
  key: string;
  code?: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

const ESC = "\x1b";
const CSI = "\x1b[";
const SS3 = "\x1bO";

/** xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl + 8·meta. */
function modifier(e: KeyLike): number {
  return 1 + (e.shiftKey ? 1 : 0) + (e.altKey ? 2 : 0) + (e.ctrlKey ? 4 : 0) + (e.metaKey ? 8 : 0);
}

const CURSOR_KEYS: Record<string, string> = {
  ArrowUp: "A",
  ArrowDown: "B",
  ArrowRight: "C",
  ArrowLeft: "D",
  Home: "H",
  End: "F",
};

const TILDE_KEYS: Record<string, number> = {
  Insert: 2,
  Delete: 3,
  PageUp: 5,
  PageDown: 6,
  F5: 15,
  F6: 17,
  F7: 18,
  F8: 19,
  F9: 20,
  F10: 21,
  F11: 23,
  F12: 24,
};

const SS3_FKEYS: Record<string, string> = { F1: "P", F2: "Q", F3: "R", F4: "S" };

/** Ctrl + key → C0 control character. */
function controlChar(key: string): string | null {
  if (key.length !== 1) return null;
  const c = key.toLowerCase();
  if (c >= "a" && c <= "z") return String.fromCharCode(c.charCodeAt(0) - 96);
  switch (c) {
    case "@":
    case " ":
    case "2":
      return "\x00";
    case "[":
    case "3":
      return ESC;
    case "\\":
    case "4":
      return "\x1c";
    case "]":
    case "5":
      return "\x1d";
    case "^":
    case "6":
      return "\x1e";
    case "_":
    case "-":
    case "7":
    case "/":
      return "\x1f";
    case "?":
    case "8":
      return "\x7f";
  }
  return null;
}

/**
 * Encode a key press as bytes for the PTY, following xterm conventions.
 *
 * Returns `null` for keys that should be left to the text input (plain
 * printable characters, so IME and dead keys work) or not sent at all.
 */
export function encodeKey(e: KeyLike, mode: number): string | null {
  const mods = modifier(e);
  const appCursor = (mode & Mode.APP_CURSOR) !== 0;

  const cursor = CURSOR_KEYS[e.key];
  if (cursor) {
    if (mods > 1) return `${CSI}1;${mods}${cursor}`;
    return (appCursor ? SS3 : CSI) + cursor;
  }

  const tilde = TILDE_KEYS[e.key];
  if (tilde !== undefined) return mods > 1 ? `${CSI}${tilde};${mods}~` : `${CSI}${tilde}~`;

  const fkey = SS3_FKEYS[e.key];
  if (fkey) return mods > 1 ? `${CSI}1;${mods}${fkey}` : SS3 + fkey;

  const alt = e.altKey ? ESC : "";
  switch (e.key) {
    case "Enter":
      return alt + "\r";
    case "Backspace":
      return alt + (e.ctrlKey ? "\x08" : "\x7f");
    case "Tab":
      return e.shiftKey ? `${CSI}Z` : alt + "\t";
    case "Escape":
      return alt + ESC;
  }

  if (e.metaKey) return null;

  if (e.ctrlKey && !e.altKey) return controlChar(e.key);
  if (e.ctrlKey && e.altKey) {
    // AltGr arrives as Ctrl+Alt on Windows/Linux; let it produce text.
    const ctrl = controlChar(e.key);
    return ctrl && e.key.length === 1 && /[a-z]/i.test(e.key) ? ESC + ctrl : null;
  }
  if (e.altKey && [...e.key].length === 1) return ESC + e.key;

  return null;
}

/** Wrap pasted text for the PTY, honouring bracketed paste mode. */
export function encodePaste(text: string, mode: number): string {
  if (mode & Mode.BRACKETED_PASTE) {
    // Strip any embedded end marker so pasted text can't break out of the bracket.
    return `${CSI}200~${text.replace(/\x1b\[201~/g, "")}${CSI}201~`;
  }
  return text.replace(/\r?\n/g, "\r");
}

export type MouseAction = "press" | "release" | "move" | "wheel-up" | "wheel-down";

export interface MouseReport {
  action: MouseAction;
  /** 0 left, 1 middle, 2 right; ignored for wheel/move-without-button. */
  button: number;
  /** Whether a button is held during `move`. */
  held: boolean;
  col: number;
  row: number;
  shift: boolean;
  alt: boolean;
  ctrl: boolean;
}

/**
 * Encode a mouse event for applications that enabled mouse reporting.
 * Returns `null` when the current mode doesn't want this event.
 */
export function encodeMouse(m: MouseReport, mode: number): string | null {
  if (!(mode & Mode.MOUSE_MODE)) return null;
  if (m.action === "move") {
    const wantsMotion = (mode & Mode.MOUSE_MOTION) !== 0 || ((mode & Mode.MOUSE_DRAG) !== 0 && m.held);
    if (!wantsMotion) return null;
  }

  let code: number;
  switch (m.action) {
    case "wheel-up":
      code = 64;
      break;
    case "wheel-down":
      code = 65;
      break;
    case "move":
      code = 32 + (m.held ? m.button : 3);
      break;
    default:
      code = m.button;
  }
  if (m.shift) code += 4;
  if (m.alt) code += 8;
  if (m.ctrl) code += 16;

  const x = m.col + 1;
  const y = m.row + 1;
  if (mode & Mode.SGR_MOUSE) {
    return `${CSI}<${code};${x};${y}${m.action === "release" ? "m" : "M"}`;
  }
  // Legacy X10/normal encoding can't express which button was released.
  if (m.action === "release") code = 3 + (code & ~3);
  if (x > 223 || y > 223) return null;
  return `${CSI}M${String.fromCharCode(32 + code, 32 + x, 32 + y)}`;
}
