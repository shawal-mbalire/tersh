// The port between the UI and whatever hosts it (the Tauri app, or a fake in
// tests): the terminal session, the user's settings, the clipboard and the window.

import type { RichMessage } from "../domain/cards";
import type { KeyLike, MouseReport } from "../domain/input";
import type { Frame, SessionInfo, SessionStats, Settings, TermEvent } from "../domain/types";

export interface SessionHandlers {
  frame(frame: Frame): void;
  rich(message: RichMessage): void;
  event(event: TermEvent): void;
}

export interface PluginSource {
  name: string;
  source: string;
  style: string | null;
}

/** Stores HTML documents for sandboxed card iframes, served from their own origin. */
export interface CardDocuments {
  put(id: string, html: string): Promise<void>;
  drop(id: string): void;
  /** URL an iframe loads the document from. */
  url(id: string): string;
}

export interface Backend {
  /** Attach to the session (starting it on first call); output arrives through `handlers`. */
  startSession(cols: number, rows: number, cellWidth: number, cellHeight: number, handlers: SessionHandlers): Promise<SessionInfo>;
  write(data: string): Promise<void>;
  /** Encode a key event against the live terminal mode in the backend and send to the PTY. */
  key?(event: KeyLike): Promise<boolean>;
  /** Encode pasted text (bracketed paste when active) in the backend and send to the PTY. */
  paste?(text: string): Promise<void>;
  /** Encode a mouse event against the live terminal mode in the backend and send to the PTY. */
  mouse?(report: MouseReport): Promise<boolean>;
  /** Handle a mouse wheel step against the live terminal mode in the backend. */
  wheel?(lines: number, col: number, row: number, shift: boolean, alt: boolean, ctrl: boolean): Promise<void>;
  /** Report focus change against the live terminal mode in the backend. */
  focus?(focused: boolean): Promise<void>;
  resize(cols: number, rows: number, cellWidth: number, cellHeight: number): Promise<void>;
  /** Scroll the viewport through history; positive is up. */
  scroll(lines: number): Promise<void>;
  scrollToBottom(): Promise<void>;
  /** The last frame received has been drawn: the session may send the next.
   *  It keeps one frame in flight, so frames never arrive faster than they are painted. */
  frameShown(): Promise<void>;
  /** Profiling counters of the session (and the host process's memory). */
  stats(): Promise<SessionStats>;

  getSettings(): Promise<Settings>;
  onSettingsChanged(handler: (settings: Settings) => void): Promise<void>;
  reloadConfig(): Promise<void>;
  /** Open the config file in the user's editor. */
  openConfig(): Promise<void>;
  listPlugins(): Promise<PluginSource[]>;

  readClipboard(): Promise<string | null>;
  writeClipboard(text: string): Promise<void>;
  setTitle(title: string): Promise<void>;
  closeWindow(): Promise<void>;
  /** The first frame is on screen: the host may now show the window. */
  windowReady(): Promise<void>;

  /** A newer version was installed over this one (e.g. by `tersh update`). */
  onAppUpdated(handler: (version: string) => void): Promise<void>;
  /** Restart into the installed version, in the same directory and size. */
  restart(): Promise<void>;

  cardDocuments: CardDocuments;
}
