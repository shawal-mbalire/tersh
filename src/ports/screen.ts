// The port between the terminal use case and the surface it draws on: the
// page (adapters/dom), or a fake in tests.

import type { RichMessage } from "../domain/cards";
import type { Palette } from "../domain/palette";
import type { Config, Frame, Metrics } from "../domain/types";

export type NotifyLevel = "info" | "error";

export interface Screen {
  readonly cols: number;
  readonly rows: number;
  readonly metrics: Metrics;
  /** Columns and rows that fit the visible area with the current font. */
  fit(): { cols: number; rows: number };
  resize(cols: number, rows: number): void;
  setFont(font: Config["font"]): void;
  restyle(palette: Palette, opacity: number): void;
  /** Show a frame from the session (painted on the next animation frame). */
  show(frame: Frame): void;
  /** Repaint everything, e.g. after a style change. */
  repaint(): void;
  /** Text of a visible row, columns `from` up to `to`. */
  rowText(y: number, from?: number, to?: number): string;
  selectionText(): string;
  clearSelection(): void;
  /** Place, update or remove a card. */
  rich(message: RichMessage): void;
  notify(message: string, level: NotifyLevel): void;
  bell(): void;
}
