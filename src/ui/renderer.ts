import type { Config, Frame, Metrics, Run } from "../domain/types";
import { CellFlag, Mode } from "../domain/types";
import { withSymbols } from "../domain/fonts";
import { BG, FG, Palette, parseHex } from "../domain/palette";

const UNDERLINES =
  CellFlag.UNDERLINE | CellFlag.DOUBLE_UNDERLINE | CellFlag.UNDERCURL | CellFlag.DOTTED_UNDERLINE | CellFlag.DASHED_UNDERLINE;

/**
 * Draws the character grid on a 2D canvas. Rows are kept as runs (as sent by
 * the core) and only rows touched by a frame are repainted. Default-background
 * cells stay transparent so the window's translucent backdrop shows through.
 */
/** Return true when `s` contains only printable ASCII (`0x20..=0x7E`), which
 *  in a monospace font is guaranteed to be 1 UTF-16 code unit and 1 cell wide
 *  without needing surrogate-pair iteration or per-character `fillText`. */
function isAscii(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    const code = s.charCodeAt(i);
    if (code < 0x20 || code > 0x7e) return false;
  }
  return true;
}

export class GridRenderer {
  cols = 0;
  rows = 0;
  metrics: Metrics = { cellWidth: 8, cellHeight: 16, baseline: 12 };
  /** Latest frame metadata (mode, cursor, scroll state). */
  frame: Frame | null = null;

  private lines: Run[][] = [];
  private dirty = new Set<number>();
  private ctx: CanvasRenderingContext2D;
  private dpr = 1;
  private fonts = new Map<number, string>();
  private currentFont = "";
  /** Alpha applied to painted cell backgrounds (from `window.opacity`). */
  private bgAlpha = 1;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly cursorEl: HTMLElement,
    private font: Config["font"],
    public palette: Palette,
    bgAlpha = 1,
  ) {
    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) throw new Error("2D canvas unavailable");
    this.ctx = ctx;
    this.bgAlpha = bgAlpha;
    this.measure();
  }

  /** Value in `window.opacity` that painted backgrounds are faded to (`0..1`). */
  get opacity(): number {
    return this.bgAlpha;
  }

  set opacity(value: number) {
    const a = Math.min(1, Math.max(0, value));
    if (a === this.bgAlpha) return;
    this.bgAlpha = a;
    this.invalidate();
  }

  setFont(font: Config["font"]) {
    this.font = font;
    this.measure();
  }

  /** Recompute cell metrics from the configured font. */
  measure(): Metrics {
    this.dpr = window.devicePixelRatio || 1;
    this.fonts.clear();
    const ctx = this.ctx;
    ctx.font = this.fontFor(0);
    const sample = ctx.measureText("M".repeat(64));
    const snap = (v: number) => Math.max(1, Math.round(v * this.dpr) / this.dpr);
    const cellWidth = snap(sample.width / 64);
    const cellHeight = snap(this.font.size * this.font.line_height);
    const ascent = sample.fontBoundingBoxAscent || this.font.size * 0.8;
    const descent = sample.fontBoundingBoxDescent || this.font.size * 0.2;
    const baseline = Math.round((cellHeight - (ascent + descent)) / 2 + ascent);
    this.metrics = { cellWidth, cellHeight, baseline };
    this.currentFont = "";
    return this.metrics;
  }

  /** Grid size that fits in the given CSS pixel area. */
  fit(width: number, height: number): { cols: number; rows: number } {
    return {
      cols: Math.max(2, Math.floor(width / this.metrics.cellWidth)),
      rows: Math.max(1, Math.floor(height / this.metrics.cellHeight)),
    };
  }

  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    const { cellWidth, cellHeight } = this.metrics;
    const width = cols * cellWidth;
    const height = rows * cellHeight;
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${height}px`;
    this.canvas.width = Math.round(width * this.dpr);
    this.canvas.height = Math.round(height * this.dpr);
    this.ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    this.ctx.textBaseline = "alphabetic";
    this.currentFont = "";
    this.lines.length = rows;
    for (let y = 0; y < rows; y++) {
      this.lines[y] ??= [];
      this.dirty.add(y);
    }
  }

  /** Merge a frame into the model. Call `draw` (once per animation frame) to paint. */
  apply(frame: Frame) {
    if (frame.cols !== this.cols || frame.rows !== this.rows) this.resize(frame.cols, frame.rows);
    if (frame.full) {
      for (let y = 0; y < this.rows; y++) {
        this.lines[y] = [];
        this.dirty.add(y);
      }
    }
    for (const row of frame.lines) {
      if (row.y >= this.rows) continue;
      this.lines[row.y] = row.runs;
      this.dirty.add(row.y);
    }
    this.frame = frame;
  }

  draw() {
    for (const y of this.dirty) this.drawRow(y);
    this.dirty.clear();
    this.drawCursor();
  }

  invalidate() {
    for (let y = 0; y < this.rows; y++) this.dirty.add(y);
  }

  /** Text of a viewport row, one character per cell (wide chars followed by a space-free gap). */
  rowText(y: number, from = 0, to = this.cols): string {
    const cells: string[] = new Array(this.cols).fill(" ");
    for (const run of this.lines[y] ?? []) {
      const chars = [...run.t];
      if (chars.length === run.n) chars.forEach((ch, i) => (cells[run.c + i] = ch));
      else {
        cells[run.c] = run.t;
        for (let i = 1; i < run.n; i++) cells[run.c + i] = "";
      }
    }
    return cells.slice(from, to).join("");
  }

  cellAt(y: number, x: number): Run | null {
    for (const run of this.lines[y] ?? []) {
      if (x >= run.c && x < run.c + run.n) return run;
    }
    return null;
  }

  private fontFor(flags: number): string {
    const key = flags & (CellFlag.BOLD | CellFlag.ITALIC);
    let font = this.fonts.get(key);
    if (!font) {
      const style = key & CellFlag.ITALIC ? "italic " : "";
      const weight = key & CellFlag.BOLD ? "bold " : "";
      font = `${style}${weight}${this.font.size}px ${withSymbols(this.font.family)}`;
      this.fonts.set(key, font);
    }
    return font;
  }

  private colors(run: Run): { fg: string; bg: string | null } {
    let fg = run.fg;
    let bg = run.bg;
    if (run.f & CellFlag.INVERSE) [fg, bg] = [bg, fg];
    const bold = (run.f & CellFlag.BOLD) !== 0;
    let fgCss = this.palette.resolve(fg, bold);
    if (run.f & CellFlag.DIM) {
      if (fg < 8) fgCss = this.palette.resolve(259 + fg);
      else if (fg === FG) fgCss = this.palette.resolve(268);
    }
    return { fg: fgCss, bg: bg === BG ? null : this.fadeBackground(this.palette.resolve(bg)) };
  }

  /** Apply `window.opacity` to a painted background colour so apps like opencode
   *  that paint their own backdrop become see-through too. */
  private fadeBackground(color: string): string {
    if (this.bgAlpha >= 1) return color;
    const rgb = parseHex(color);
    if (!rgb) return color;
    const [r, g, b] = rgb;
    return `rgba(${r}, ${g}, ${b}, ${this.bgAlpha})`;
  }

  private drawRow(y: number) {
    const ctx = this.ctx;
    const { cellWidth: cw, cellHeight: ch, baseline } = this.metrics;
    const top = y * ch;
    ctx.clearRect(0, top, this.cols * cw, ch);

    for (const run of this.lines[y] ?? []) {
      const { fg, bg } = this.colors(run);
      const left = run.c * cw;
      const width = run.n * cw;
      if (bg) {
        ctx.fillStyle = bg;
        ctx.fillRect(left, top, width, ch);
      }
      if (run.f & CellFlag.HIDDEN) continue;

      const font = this.fontFor(run.f);
      if (font !== this.currentFont) {
        ctx.font = font;
        this.currentFont = font;
      }
      ctx.fillStyle = fg;
      if (run.n === 1) {
        if (run.t !== " ") ctx.fillText(run.t, left, top + baseline);
      } else if (run.t.length === run.n && isAscii(run.t)) {
        if (run.t.trim().length > 0) ctx.fillText(run.t, left, top + baseline);
      } else {
        const chars = [...run.t];
        if (chars.length === run.n) {
          for (let i = 0; i < chars.length; i++) {
            if (chars[i] !== " ") ctx.fillText(chars[i], left + i * cw, top + baseline);
          }
        } else {
          ctx.fillText(run.t, left, top + baseline);
        }
      }

      if (run.f & (UNDERLINES | CellFlag.STRIKEOUT)) this.decorate(run.f, fg, left, width, top);
    }
  }

  private decorate(flags: number, color: string, left: number, width: number, top: number) {
    const ctx = this.ctx;
    const { cellHeight: ch, baseline } = this.metrics;
    const thickness = Math.max(1, Math.round(this.font.size / 14));
    const underlineY = Math.min(top + ch - thickness, top + baseline + 2 * thickness) + 0.5;
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineWidth = thickness;
    ctx.beginPath();
    if (flags & CellFlag.UNDERCURL) {
      const amp = Math.max(1, thickness);
      for (let x = left; x <= left + width; x += 1) {
        const yy = underlineY + Math.sin(((x - left) / 3) * Math.PI) * amp;
        if (x === left) ctx.moveTo(x, yy);
        else ctx.lineTo(x, yy);
      }
    } else if (flags & UNDERLINES) {
      if (flags & CellFlag.DOTTED_UNDERLINE) ctx.setLineDash([thickness, thickness * 2]);
      if (flags & CellFlag.DASHED_UNDERLINE) ctx.setLineDash([thickness * 4, thickness * 2]);
      ctx.moveTo(left, underlineY);
      ctx.lineTo(left + width, underlineY);
      if (flags & CellFlag.DOUBLE_UNDERLINE) {
        ctx.moveTo(left, underlineY - 2 * thickness);
        ctx.lineTo(left + width, underlineY - 2 * thickness);
      }
    }
    if (flags & CellFlag.STRIKEOUT) {
      const y = Math.round(top + ch / 2) + 0.5;
      ctx.moveTo(left, y);
      ctx.lineTo(left + width, y);
    }
    ctx.stroke();
    ctx.restore();
  }

  private drawCursor() {
    const frame = this.frame;
    const el = this.cursorEl;
    if (!frame || frame.cursor.shape === "hidden" || !(frame.mode & Mode.SHOW_CURSOR)) {
      el.hidden = true;
      return;
    }
    const { cellWidth: cw, cellHeight: ch } = this.metrics;
    const { x, y, shape, blink } = frame.cursor;
    const run = this.cellAt(y, x);
    const wide = run && run.c === x && run.n === 2 && [...run.t].length !== run.n;
    el.hidden = false;
    el.className = `shape-${shape}${blink ? " blink" : ""}`;
    el.style.transform = `translate(${x * cw}px, ${y * ch}px)`;
    el.style.width = `${(wide ? 2 : 1) * cw}px`;
    el.style.height = `${ch}px`;
    // A block cursor shows the character underneath in the background colour.
    if (shape === "block") {
      const text = run ? this.charAt(run, x) : "";
      el.textContent = text.trim() ? text : "";
      el.style.font = this.fontFor(run?.f ?? 0);
      el.style.lineHeight = `${ch}px`;
    } else {
      el.textContent = "";
    }
  }

  private charAt(run: Run, x: number): string {
    const chars = [...run.t];
    return chars.length === run.n ? (chars[x - run.c] ?? "") : run.t;
  }
}
