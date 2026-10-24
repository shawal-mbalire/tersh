import { describe, expect, it } from "vitest";
import { encodeKey, encodeMouse, encodePaste, type KeyLike, type MouseReport, normalizeTyped } from "../domain/input";
import { Mode } from "../domain/types";

const key = (k: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key: k,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("encodeKey", () => {
  it("leaves plain text to the input element", () => {
    expect(encodeKey(key("a"), 0)).toBeNull();
    expect(encodeKey(key("A", { shiftKey: true }), 0)).toBeNull();
  });

  it("encodes control characters", () => {
    expect(encodeKey(key("c", { ctrlKey: true }), 0)).toBe("\x03");
    expect(encodeKey(key("[", { ctrlKey: true }), 0)).toBe("\x1b");
    expect(encodeKey(key(" ", { ctrlKey: true }), 0)).toBe("\x00");
  });

  it("prefixes alt with escape", () => {
    expect(encodeKey(key("x", { altKey: true }), 0)).toBe("\x1bx");
    expect(encodeKey(key("Enter", { altKey: true }), 0)).toBe("\x1b\r");
  });

  it("honours application cursor mode", () => {
    expect(encodeKey(key("ArrowUp"), 0)).toBe("\x1b[A");
    expect(encodeKey(key("ArrowUp"), Mode.APP_CURSOR)).toBe("\x1bOA");
    expect(encodeKey(key("ArrowLeft", { ctrlKey: true }), Mode.APP_CURSOR)).toBe("\x1b[1;5D");
  });

  it("encodes editing and function keys", () => {
    expect(encodeKey(key("Backspace"), 0)).toBe("\x7f");
    expect(encodeKey(key("Tab", { shiftKey: true }), 0)).toBe("\x1b[Z");
    expect(encodeKey(key("Delete"), 0)).toBe("\x1b[3~");
    expect(encodeKey(key("PageUp", { shiftKey: true }), 0)).toBe("\x1b[5;2~");
    expect(encodeKey(key("F1"), 0)).toBe("\x1bOP");
    expect(encodeKey(key("F5", { ctrlKey: true }), 0)).toBe("\x1b[15;5~");
  });

  it("lets AltGr produce text", () => {
    expect(encodeKey(key("@", { ctrlKey: true, altKey: true }), 0)).toBeNull();
  });
});

describe("encodePaste", () => {
  it("converts newlines without bracketed paste", () => {
    expect(encodePaste("a\nb\r\nc", 0)).toBe("a\rb\rc");
  });

  it("brackets and neutralises embedded end markers", () => {
    expect(encodePaste("x\x1b[201~y", Mode.BRACKETED_PASTE)).toBe("\x1b[200~xy\x1b[201~");
  });
});

describe("encodeMouse", () => {
  const base: MouseReport = { action: "press", button: 0, held: false, col: 4, row: 1, shift: false, alt: false, ctrl: false };

  it("ignores mouse when not requested", () => {
    expect(encodeMouse(base, 0)).toBeNull();
  });

  it("uses SGR encoding when enabled", () => {
    const mode = Mode.MOUSE_REPORT_CLICK | Mode.SGR_MOUSE;
    expect(encodeMouse(base, mode)).toBe("\x1b[<0;5;2M");
    expect(encodeMouse({ ...base, action: "release" }, mode)).toBe("\x1b[<0;5;2m");
    expect(encodeMouse({ ...base, action: "wheel-down", ctrl: true }, mode)).toBe("\x1b[<81;5;2M");
  });

  it("only reports motion in the matching mode", () => {
    const drag = { ...base, action: "move" as const };
    expect(encodeMouse(drag, Mode.MOUSE_REPORT_CLICK | Mode.SGR_MOUSE)).toBeNull();
    expect(encodeMouse({ ...drag, held: true }, Mode.MOUSE_DRAG | Mode.SGR_MOUSE)).toBe("\x1b[<32;5;2M");
    expect(encodeMouse(drag, Mode.MOUSE_MOTION | Mode.SGR_MOUSE)).toBe("\x1b[<35;5;2M");
  });

  it("falls back to legacy encoding", () => {
    expect(encodeMouse(base, Mode.MOUSE_REPORT_CLICK)).toBe("\x1b[M" + String.fromCharCode(32, 37, 34));
    expect(encodeMouse({ ...base, action: "release", button: 2 }, Mode.MOUSE_REPORT_CLICK)).toBe(
      "\x1b[M" + String.fromCharCode(35, 37, 34),
    );
  });
});

describe("normalizeTyped", () => {
  it("turns a no-break space from the space bar into a space", () => {
    expect(normalizeTyped("\u00a0", true)).toBe(" ");
    expect(normalizeTyped("a\u00a0b", true)).toBe("a b");
  });

  it("keeps a deliberately typed no-break space", () => {
    expect(normalizeTyped("\u00a0", false)).toBe("\u00a0");
  });
});
