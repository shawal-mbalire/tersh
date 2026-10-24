import type { Theme } from "./types";
import { RGB_FLAG } from "./types";

export const FG = 256;
export const BG = 257;
export const CURSOR = 258;
const DIM_BASE = 259;
const BRIGHT_FG = 267;
const DIM_FG = 268;

type Rgb = [number, number, number];

export function parseHex(color: string): Rgb | null {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color.trim());
  if (!m) return null;
  let hex = m[1];
  if (hex.length === 3) hex = [...hex].map((c) => c + c).join("");
  const n = parseInt(hex, 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

const css = ([r, g, b]: Rgb) => `rgb(${r},${g},${b})`;
const dim = ([r, g, b]: Rgb): Rgb => [Math.round(r * 0.66), Math.round(g * 0.66), Math.round(b * 0.66)];

/** Resolves encoded cell colours (palette index or flagged RGB) to CSS colours. */
export class Palette {
  private readonly colors: string[] = new Array(269);
  private readonly rgbCache = new Map<number, string>();

  constructor(
    theme: Pick<Theme, "foreground" | "background" | "cursor" | "palette"> & Partial<Theme>,
    readonly boldIsBright = false,
  ) {
    const fallback = parseHex("#d4d4d4")!;
    const ansi = Array.from({ length: 16 }, (_, i) => parseHex(theme.palette[i] ?? "") ?? fallback);
    const fg = parseHex(theme.foreground) ?? fallback;
    const bg = parseHex(theme.background) ?? [0, 0, 0];

    for (let i = 0; i < 16; i++) this.colors[i] = css(ansi[i]);
    const level = (v: number) => (v === 0 ? 0 : 55 + v * 40);
    for (let i = 16; i < 232; i++) {
      const n = i - 16;
      this.colors[i] = css([level(Math.floor(n / 36)), level(Math.floor(n / 6) % 6), level(n % 6)]);
    }
    for (let i = 232; i < 256; i++) {
      const v = 8 + (i - 232) * 10;
      this.colors[i] = css([v, v, v]);
    }
    this.colors[FG] = css(fg);
    this.colors[BG] = css(bg);
    this.colors[CURSOR] = css(parseHex(theme.cursor) ?? fg);
    for (let i = 0; i < 8; i++) this.colors[DIM_BASE + i] = css(dim(ansi[i]));
    this.colors[BRIGHT_FG] = css(fg);
    this.colors[DIM_FG] = css(dim(fg));
  }

  resolve(color: number, bold = false): string {
    if (color & RGB_FLAG) {
      let cached = this.rgbCache.get(color);
      if (!cached) {
        cached = `#${(color & 0xffffff).toString(16).padStart(6, "0")}`;
        if (this.rgbCache.size > 4096) this.rgbCache.clear();
        this.rgbCache.set(color, cached);
      }
      return cached;
    }
    if (bold && this.boldIsBright && color < 8) color += 8;
    return this.colors[color] ?? this.colors[FG];
  }
}
