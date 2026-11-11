// Mouse wheel: how far it scrolled, in lines, and what that means for the program.

import { Mode } from "./types";

/** `WheelEvent.deltaMode` values. */
export const DELTA_LINE = 1;
export const DELTA_PAGE = 2;

export interface WheelDelta {
  deltaY: number;
  deltaMode: number;
}

/**
 * Whole lines scrolled, carrying the fraction over to the next event so slow
 * trackpad scrolling still moves. Positive is down.
 */
export function wheelLines(
  { deltaY, deltaMode }: WheelDelta,
  remainder: number,
  rows: number,
  cellHeight: number,
): { lines: number; remainder: number } {
  const delta = deltaMode === DELTA_LINE ? deltaY : deltaMode === DELTA_PAGE ? deltaY * rows : deltaY / cellHeight;
  const total = remainder + delta;
  const lines = Math.trunc(total);
  return { lines, remainder: total - lines };
}

/** What a wheel movement does: report it to the program, send arrow keys, or scroll history. */
export type WheelAction = "report" | "keys" | "history";

export function wheelAction(mode: number, reportsMouse: boolean): WheelAction {
  if (reportsMouse) return "report";
  if (mode & Mode.ALT_SCREEN && mode & Mode.ALTERNATE_SCROLL) return "keys";
  return "history";
}

/** Arrow keys for `lines` of wheel movement on the alternate screen (negative is up). */
export function wheelKeys(lines: number, mode: number): string {
  const key = lines < 0 ? "A" : "B";
  const prefix = mode & Mode.APP_CURSOR ? "\x1bO" : "\x1b[";
  return (prefix + key).repeat(Math.abs(lines));
}
