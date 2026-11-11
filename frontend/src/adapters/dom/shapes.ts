// Powerline separators and block elements drawn as vector shapes sized to the
// cell, instead of font glyphs: a font draws them at its own em height, which
// leaves gaps and misaligned caps whenever line height isn't exactly 1.

type Painter = (ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number) => void;

const rect =
  (fx: number, fy: number, fw: number, fh: number): Painter =>
  (ctx, x, y, w, h) =>
    ctx.fillRect(x + fx * w, y + fy * h, fw * w, fh * h);

/** Path for a Powerline triangle or half-circle; `left` points it leftward. */
function separator(round: boolean, left: boolean, solid: boolean): Painter {
  return (ctx, x, y, w, h) => {
    const base = left ? x + w : x;
    const tip = left ? x : x + w;
    ctx.beginPath();
    if (round) {
      ctx.ellipse(base, y + h / 2, w, h / 2, 0, -Math.PI / 2, Math.PI / 2, left);
    } else {
      ctx.moveTo(base, y);
      ctx.lineTo(tip, y + h / 2);
      ctx.lineTo(base, y + h);
    }
    if (solid) {
      ctx.closePath();
      ctx.fill();
    } else {
      ctx.save();
      ctx.strokeStyle = ctx.fillStyle;
      ctx.lineWidth = Math.max(1, w / 8);
      ctx.stroke();
      ctx.restore();
    }
  };
}

const SHAPES = new Map<string, Painter>([
  // Powerline:
  ["\u{e0b0}", separator(false, false, true)],
  ["\u{e0b1}", separator(false, false, false)],
  ["\u{e0b2}", separator(false, true, true)],
  ["\u{e0b3}", separator(false, true, false)],
  ["\u{e0b4}", separator(true, false, true)],
  ["\u{e0b5}", separator(true, false, false)],
  ["\u{e0b6}", separator(true, true, true)],
  ["\u{e0b7}", separator(true, true, false)],
  // Block elements.
  ["▀", rect(0, 0, 1, 1 / 2)],
  ["▄", rect(0, 1 / 2, 1, 1 / 2)],
  ["█", rect(0, 0, 1, 1)],
  ["▌", rect(0, 0, 1 / 2, 1)],
  ["▐", rect(1 / 2, 0, 1 / 2, 1)],
  ["▔", rect(0, 0, 1, 1 / 8)],
  ["▕", rect(7 / 8, 0, 1 / 8, 1)],
]);
// Lower eighths ▁..▇ and left eighths ▉..▏.
for (let i = 1; i <= 7; i++) {
  SHAPES.set(String.fromCharCode(0x2580 + i), rect(0, 1 - i / 8, 1, i / 8));
  SHAPES.set(String.fromCharCode(0x2588 + i), rect(0, 0, (8 - i) / 8, 1));
}

/** Paint `ch` as a shape filling its cell, in the current fill style. Returns false if it isn't one. */
export function drawShape(ctx: CanvasRenderingContext2D, ch: string, x: number, y: number, w: number, h: number): boolean {
  const paint = SHAPES.get(ch);
  if (!paint) return false;
  paint(ctx, x, y, w, h);
  return true;
}
