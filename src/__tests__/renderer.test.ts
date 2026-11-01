// The grid renderer on a fake 2D context (jsdom has no canvas): text must
// land on the cell grid whatever the font's own character widths are.

import { describe, expect, it } from "vitest";
import { GridRenderer } from "../ui/renderer";
import { Palette } from "../domain/palette";
import type { Frame, Run } from "../domain/types";

interface Draw {
  text: string;
  x: number;
}

/** A 2D context whose font has the given per-character advances (default 9px). */
function fakeContext(widths: Record<string, number>, letterSpacing = true) {
  const draws: Draw[] = [];
  const width = (text: string) => [...text].reduce((sum, ch) => sum + (widths[ch] ?? 9), 0);
  const ctx: Record<string, unknown> = {
    font: "",
    fillStyle: "",
    textBaseline: "alphabetic",
    measureText: (text: string) => ({ width: width(text), fontBoundingBoxAscent: 11, fontBoundingBoxDescent: 3 }),
    fillText: (text: string, x: number) => void draws.push({ text, x }),
  };
  if (letterSpacing) ctx.letterSpacing = "0px";
  for (const name of [
    "setTransform",
    "fillRect",
    "clearRect",
    "save",
    "restore",
    "beginPath",
    "moveTo",
    "lineTo",
    "stroke",
    "setLineDash",
    "fill",
    "closePath",
    "ellipse",
  ]) {
    ctx[name] = () => undefined;
  }
  return { ctx, draws };
}

function renderer(widths: Record<string, number>, letterSpacing = true) {
  const { ctx, draws } = fakeContext(widths, letterSpacing);
  const canvas = document.createElement("canvas");
  (canvas as unknown as { getContext: () => unknown }).getContext = () => ctx;
  const palette = new Palette({ foreground: "#ffffff", background: "#000000", cursor: "#ffffff", palette: [] });
  const font = { family: "test", size: 15, line_height: 1.2, bold_is_bright: false };
  return { r: new GridRenderer(canvas, document.createElement("div"), font, palette), draws };
}

const run = (c: number, t: string): Run => ({ c, n: [...t].length, t, fg: 256, bg: 257, f: 0 });

const frame = (runs: Run[]): Frame => ({
  cols: 40,
  rows: 1,
  full: true,
  lines: [{ y: 0, runs }],
  cursor: { x: 0, y: 0, shape: "block", blink: false },
  mode: 0,
  alt: false,
  display_offset: 0,
  history: 0,
  scroll_base: 0,
});

describe("GridRenderer", () => {
  it("draws a run of a monospace font in one call", () => {
    const { r, draws } = renderer({});
    r.resize(40, 1);
    r.apply(frame([run(2, "third line here")]));
    r.draw();
    expect(draws).toEqual([{ text: "third line here", x: 18 }]);
  });

  it("puts each character in its own cell when the font has narrower letters", () => {
    // Sono's `i` is a pixel narrower than its other letters.
    const { r, draws } = renderer({ i: 8 });
    r.resize(40, 1);
    r.apply(frame([run(2, "third line here")]));
    r.draw();
    const cell = (x: number) => Math.floor(x / 9);
    // One draw per non-space character, each inside its column; the last `e` in column 16.
    expect(draws.map((d) => d.text).join("")).toBe("thirdlinehere");
    expect(draws.map((d) => cell(d.x))).toEqual([2, 3, 4, 5, 6, 8, 9, 10, 11, 13, 14, 15, 16]);
    // Narrow glyphs are centred in their cell.
    expect(draws[2]).toEqual({ text: "i", x: 4 * 9 + 0.5 });
  });

  it("falls back to per-cell drawing without letter spacing when the advance isn't a whole cell", () => {
    // A 9.4px advance snaps to a 9px cell; without letterSpacing the run would drift.
    const all: Record<string, number> = {};
    for (let code = 0x20; code < 0x7f; code++) all[String.fromCharCode(code)] = 9.4;
    const { r, draws } = renderer(all, false);
    r.resize(40, 1);
    r.apply(frame([run(0, "abc")]));
    r.draw();
    expect(draws.map((d) => d.x)).toEqual([0, 9, 18]);
  });
});
