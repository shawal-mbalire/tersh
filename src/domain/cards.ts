// Rich-card messages and where cards go on the grid.

import type { Frame, Metrics } from "./types";

/** Where a card sits. Grid anchors are resolved by the core (see `rich.rs`). */
export interface Anchor {
  mode?: "cursor" | "cell" | "float";
  screen?: "primary" | "alt";
  /** Absolute line on the primary screen, screen row on the alternate one. */
  line?: number;
  col?: number;
  /** Size in cells; omitted means "to the right edge" / "fit content". */
  rows?: number;
  cols?: number;
  /** Float anchors: numbers are cells, strings are CSS lengths ("10%", "120px"). */
  top?: number | string;
  left?: number | string;
  width?: number | string;
  height?: number | string;
}

export interface RichMessage {
  op?: "show" | "update" | "move" | "close" | "clear";
  id?: string;
  kind?: string;
  title?: string;
  anchor?: Anchor;
  data?: unknown;
  /** Show the header bar (title + close button). Default true. */
  chrome?: boolean;
  /** Keep the card but don't display it (e.g. its buffer line scrolled out of an editor window). */
  hidden?: boolean;
  /** For `clear`: which cards to remove. Default all. */
  screen?: "primary" | "alt";
  [key: string]: unknown;
}

export interface Geometry {
  metrics: Metrics;
  frame: Frame;
}

/** Pixel box of a card relative to the grid's top-left, or null when off-screen. */
export function cardBox(anchor: Anchor, geo: Geometry): { left: number; top: number; width: number | null; height: number | null } | null {
  const { frame, metrics } = geo;
  const { cellWidth: cw, cellHeight: ch } = metrics;
  const gridWidth = frame.cols * cw;
  const gridHeight = frame.rows * ch;

  if (anchor.mode === "float") {
    const len = (v: number | string | undefined, cell: number, fallback: string) =>
      v === undefined ? fallback : typeof v === "number" ? `${v * cell}px` : v;
    const px = (v: string, total: number) => (v.endsWith("%") ? (parseFloat(v) / 100) * total : parseFloat(v) || 0);
    return {
      left: px(len(anchor.left, cw, "10%"), gridWidth),
      top: px(len(anchor.top, ch, "10%"), gridHeight),
      width: px(len(anchor.width, cw, "80%"), gridWidth),
      height: anchor.height === undefined ? null : px(len(anchor.height, ch, "0"), gridHeight),
    };
  }

  const screen = anchor.screen ?? "primary";
  if ((screen === "alt") !== frame.alt) return null;
  const line = anchor.line ?? 0;
  const row = screen === "primary" ? line - (frame.scroll_base - frame.display_offset) : line;
  const col = Math.min(anchor.col ?? 0, frame.cols - 1);
  const rows = anchor.rows;
  if (rows !== undefined && row + rows <= 0) return null;
  if (row >= frame.rows) return null;
  const cols = anchor.cols ?? frame.cols - col;
  return {
    left: col * cw,
    top: row * ch,
    width: Math.min(cols, frame.cols - col) * cw,
    height: rows === undefined ? null : rows * ch,
  };
}

/** Whether a primary-screen card has scrolled out of the history entirely. */
export function isExpired(anchor: Anchor, frame: Frame): boolean {
  if (anchor.mode === "float" || (anchor.screen ?? "primary") !== "primary") return false;
  const rows = anchor.rows ?? 1;
  return (anchor.line ?? 0) + rows <= frame.scroll_base - frame.history;
}
