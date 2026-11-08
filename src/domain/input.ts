import { Mode } from "./types";

export interface KeyLike {
  key: string;
  code?: string;
  location?: number;
  repeat?: boolean;
  type?: string;
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

const KITTY_FUNCTIONAL_KEYS: Record<string, number> = {
  Escape: 27,
  Enter: 13,
  Tab: 9,
  Backspace: 127,
  CapsLock: 57358,
  ScrollLock: 57359,
  NumLock: 57360,
  PrintScreen: 57361,
  Pause: 57362,
  ContextMenu: 57363,
  F13: 57376,
  F14: 57377,
  F15: 57378,
  F16: 57379,
  F17: 57380,
  F18: 57381,
  F19: 57382,
  F20: 57383,
  F21: 57384,
  F22: 57385,
  F23: 57386,
  F24: 57387,
};

const KITTY_MODIFIER_CODES: Record<string, number> = {
  ShiftLeft: 57441,
  ControlLeft: 57442,
  AltLeft: 57443,
  MetaLeft: 57444,
  ShiftRight: 57447,
  ControlRight: 57448,
  AltRight: 57449,
  MetaRight: 57450,
  AltGraph: 57453,
};

const CODE_BASE_CHAR: Record<string, string> = {
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

/**
 * Resolve the logical key name when WebKitGTK reports `"Unidentified"` for XKB
 * keysyms such as `ISO_Left_Tab` (`Shift+Tab` on Linux, where `code` is `"Tab"`).
 */
export function effectiveKey(e: Pick<KeyLike, "key" | "code" | "shiftKey">): string {
  if (e.key && e.key !== "Unidentified") return e.key;
  const code = e.code;
  if (!code) return e.key;
  switch (code) {
    case "Tab":
      return "Tab";
    case "Enter":
    case "NumpadEnter":
      return "Enter";
    case "Backspace":
      return "Backspace";
    case "Escape":
      return "Escape";
    case "Space":
      return " ";
  }
  if (/^Key[A-Z]$/.test(code)) return e.shiftKey ? code[3] : code[3].toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code[5];
  if (CODE_BASE_CHAR[code]) return CODE_BASE_CHAR[code];
  if (
    CURSOR_KEYS[code] !== undefined ||
    TILDE_KEYS[code] !== undefined ||
    SS3_FKEYS[code] !== undefined ||
    KITTY_FUNCTIONAL_KEYS[code] !== undefined
  ) {
    return code;
  }
  return e.key;
}

function modifierKeyCode(e: KeyLike): number | null {
  if (e.code && KITTY_MODIFIER_CODES[e.code] !== undefined) return KITTY_MODIFIER_CODES[e.code];
  const right = e.location === 2;
  switch (e.key) {
    case "Shift":
      return right ? 57447 : 57441;
    case "Control":
      return right ? 57448 : 57442;
    case "Alt":
      return right ? 57449 : 57443;
    case "Meta":
    case "Super":
      return right ? 57450 : 57444;
    case "AltGraph":
      return 57453;
    default:
      return null;
  }
}

function baseAndShiftedCode(e: KeyLike): { base: number; shifted?: number } {
  const cp = e.key.codePointAt(0)!;
  let baseChar = [...e.key.toLowerCase()][0];
  if (e.code) {
    if (/^Key[A-Z]$/.test(e.code)) {
      baseChar = e.code[3].toLowerCase();
    } else if (/^Digit[0-9]$/.test(e.code)) {
      baseChar = e.code[5];
    } else if (CODE_BASE_CHAR[e.code]) {
      baseChar = CODE_BASE_CHAR[e.code];
    }
  }
  const base = baseChar.codePointAt(0)!;
  const shifted = e.shiftKey && cp !== base ? cp : undefined;
  return { base, shifted };
}

function formatKittyU(keyParam: string | number, mods: number, eventSuffix: string, associatedText?: number): string {
  const modPart = mods > 1 || eventSuffix ? `${mods}${eventSuffix}` : "";
  if (associatedText !== undefined) {
    return `${CSI}${keyParam};${modPart};${associatedText}u`;
  }
  return modPart ? `${CSI}${keyParam};${modPart}u` : `${CSI}${keyParam}u`;
}

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
 * Encode a key press (or release) as bytes for the PTY, following xterm and
 * Kitty keyboard protocol conventions (matching `alacritty`).
 *
 * Returns `null` for keys that should be left to the text input (plain
 * printable characters, so IME and dead keys work) or not sent at all.
 */
export function encodeKey(e: KeyLike, mode: number): string | null {
  const k = effectiveKey(e);
  if (k !== e.key) e = { ...e, key: k };
  const eventType = e.type === "keyup" ? 3 : e.repeat ? 2 : 1;
  const reportEvents = (mode & Mode.REPORT_EVENT_TYPES) !== 0;
  if (eventType === 3 && !reportEvents) return null;

  const disambiguate = (mode & Mode.DISAMBIGUATE_KEYS) !== 0;
  const reportAlt = (mode & Mode.REPORT_ALTERNATE_KEYS) !== 0;
  const reportAll = (mode & Mode.REPORT_ALL_KEYS_AS_ESC) !== 0;
  const reportText = (mode & Mode.REPORT_ASSOCIATED_TEXT) !== 0;
  const kittyActive = (mode & Mode.KITTY_KEYBOARD_PROTOCOL) !== 0;
  const eventSuffix = reportEvents && eventType !== 1 ? `:${eventType}` : "";

  const mods = modifier(e);
  const appCursor = (mode & Mode.APP_CURSOR) !== 0;

  const modCode = modifierKeyCode(e);
  if (modCode !== null) {
    if (!reportAll) return null;
    return formatKittyU(modCode, mods, eventSuffix);
  }

  const cursor = CURSOR_KEYS[e.key];
  if (cursor) {
    if (mods > 1 || eventSuffix) return `${CSI}1;${mods}${eventSuffix}${cursor}`;
    return (appCursor ? SS3 : CSI) + cursor;
  }

  const tilde = TILDE_KEYS[e.key];
  if (tilde !== undefined) {
    return mods > 1 || eventSuffix ? `${CSI}${tilde};${mods}${eventSuffix}~` : `${CSI}${tilde}~`;
  }

  const fkey = SS3_FKEYS[e.key];
  if (fkey) {
    return mods > 1 || eventSuffix ? `${CSI}1;${mods}${eventSuffix}${fkey}` : SS3 + fkey;
  }

  const functional = KITTY_FUNCTIONAL_KEYS[e.key];
  if (functional !== undefined) {
    const isBasicKey = e.key === "Enter" || e.key === "Tab" || e.key === "Backspace";
    if (
      reportAll ||
      (!isBasicKey && (disambiguate || kittyActive)) ||
      (isBasicKey && (disambiguate || reportEvents) && (mods > 1 || eventSuffix !== ""))
    ) {
      return formatKittyU(functional, mods, eventSuffix);
    }
    if (eventType === 3) return null;
  }

  const chars = [...e.key];
  if (chars.length === 1) {
    // AltGr arrives as Ctrl+Alt on Windows/Linux; let it produce text unless
    // a Kitty mode explicitly asks for all keys as escape sequences.
    const isAltGrText = e.ctrlKey && e.altKey && !e.code && controlChar(e.key) === null && !/[a-z ]/i.test(e.key);
    if (!isAltGrText) {
      const hasNonShiftMod = e.ctrlKey || e.altKey || e.metaKey;
      const isShiftSpace = e.shiftKey && e.key === " ";
      if (reportAll || ((disambiguate || reportEvents) && (hasNonShiftMod || isShiftSpace || eventSuffix !== ""))) {
        const { base, shifted } = baseAndShiftedCode(e);
        const keyParam = reportAlt && shifted !== undefined ? `${base}:${shifted}` : base;
        const textCode = reportText && eventType !== 3 && !hasNonShiftMod ? e.key.codePointAt(0)! : undefined;
        return formatKittyU(keyParam, mods, eventSuffix, textCode);
      }
    }
  }

  if (eventType === 3) return null;

  const alt = e.altKey ? ESC : "";
  switch (e.key) {
    case "Enter":
      return alt + (mode & Mode.LINE_FEED_NEW_LINE ? "\r\n" : "\r");
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
    return ctrl && e.key.length === 1 && /[a-z ]/i.test(e.key) ? ESC + ctrl : null;
  }
  if (e.altKey && chars.length === 1) return ESC + e.key;

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
  if (mode & Mode.UTF8_MOUSE) {
    if (x > 2015 || y > 2015) return null;
    return `${CSI}M${String.fromCodePoint(32 + code, 32 + x, 32 + y)}`;
  }
  if (x > 223 || y > 223) return null;
  return `${CSI}M${String.fromCharCode(32 + code, 32 + x, 32 + y)}`;
}

/**
 * Text typed with the space bar must reach the shell as a plain space. Some
 * WebKit editing paths insert U+00A0 (no-break space) instead, which shells
 * treat as part of a word (`ls ` is "command not found").
 */
export function normalizeTyped(text: string, spaceKey: boolean): string {
  return spaceKey ? text.replace(/ /g, " ") : text;
}
