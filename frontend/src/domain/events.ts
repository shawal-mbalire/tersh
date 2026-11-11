// Events the terminal announces to plugins.

import type { Config, Frame } from "./types";

export interface Disposable {
  dispose(): void;
}

export interface TershEvents {
  frame: Frame;
  /** The config was reloaded (after the terminal restyled itself). */
  config: Config;
  title: string;
  bell: undefined;
  exit: number | null;
  resize: { cols: number; rows: number };
  focus: boolean;
}
