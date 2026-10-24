import { describe, expect, it } from "vitest";
import { pickMime } from "../cards/renderers/notebook";
import { ansiToHtml } from "../cards/renderers/ansi";
import { imageSource, joinText } from "../cards/renderers/util";
import { comboFromEvent, normalizeCombo } from "../plugins/api";
import { Palette } from "../term/palette";
import { RGB_FLAG } from "../types";

describe("keybinding combos", () => {
  it("normalises modifier order and aliases", () => {
    expect(normalizeCombo("Shift+Ctrl+K")).toBe("ctrl+shift+k");
    expect(normalizeCombo("cmd+option+Enter")).toBe("alt+meta+enter");
    const event = { key: "K", ctrlKey: true, shiftKey: true, altKey: false, metaKey: false };
    expect(comboFromEvent(event)).toBe(normalizeCombo("ctrl+shift+k"));
  });
});

describe("notebook output", () => {
  it("prefers rich MIME types", () => {
    expect(pickMime({ "text/plain": "x", "image/png": "..." })).toBe("image/png");
    expect(pickMime({ "text/plain": "x", "text/html": "<b>" })).toBe("text/html");
    expect(pickMime({ "application/x-unknown": 1 })).toBeNull();
  });

  it("converts ANSI tracebacks", () => {
    expect(ansiToHtml("\x1b[31mError\x1b[0m: <x>")).toBe('<span class="ansi-red">Error</span>: &lt;x&gt;');
  });

  it("builds image sources", () => {
    expect(imageSource("iVBOR\nw0=", "image/png")).toBe("data:image/png;base64,iVBORw0=");
    expect(imageSource({ data: "AAA", mime: "image/jpeg" })).toBe("data:image/jpeg;base64,AAA");
    expect(imageSource("data:image/gif;base64,R0")).toBe("data:image/gif;base64,R0");
    expect(joinText(["a", "b"])).toBe("ab");
  });
});

describe("palette", () => {
  const palette = new Palette(
    {
      foreground: "#ffffff",
      background: "#000000",
      cursor: "#ff0000",
      selection: "#333",
      palette: ["#000000", "#aa0000", "#00aa00", "#aaaa00", "#0000aa", "#aa00aa", "#00aaaa", "#aaaaaa"],
    },
    true,
  );

  it("resolves indices, true colour and bold-as-bright", () => {
    expect(palette.resolve(1)).toBe("rgb(170,0,0)");
    expect(palette.resolve(RGB_FLAG | 0x0a0b0c)).toBe("#0a0b0c");
    expect(palette.resolve(16 + 36 * 5)).toBe("rgb(255,0,0)");
    expect(palette.resolve(232)).toBe("rgb(8,8,8)");
    expect(palette.resolve(256)).toBe("rgb(255,255,255)");
  });
});
