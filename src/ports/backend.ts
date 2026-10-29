// The port between the UI and whatever hosts it (the Tauri app, or a fake in
// tests): the terminal session, the user's settings, the clipboard and the window.

import type { RichMessage } from "../domain/cards";
import type { Frame, SessionInfo, Settings, TermEvent } from "../domain/types";

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
  resize(cols: number, rows: number, cellWidth: number, cellHeight: number): Promise<void>;
  /** Scroll the viewport through history; positive is up. */
  scroll(lines: number): Promise<void>;
  scrollToBottom(): Promise<void>;

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

  /** A newer version was installed over this one (e.g. by `tersh update`). */
  onAppUpdated(handler: (version: string) => void): Promise<void>;
  /** Restart into the installed version, in the same directory and size. */
  restart(): Promise<void>;

  cardDocuments: CardDocuments;
}
