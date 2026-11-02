// The API handed to plugins: the use case's actions, keys and events, plus
// what only the page has (card renderers, the grid, styles, toasts).

import { version as packageVersion } from "../../../../package.json";
import type { ActionHandler as AppActionHandler, KeyHandler as AppKeyHandler, RichHandler, TerminalApp } from "../../../app/terminal";
import type { Backend } from "../../../ports/backend";
import type { DomScreen } from "../terminal";
import type { CardRenderer, RichMessage } from "../cards/manager";
import type { Config, SessionStats } from "../../../domain/types";
import type { UiProfile } from "../../../domain/profile";
import type { Disposable, TershEvents } from "../../../domain/events";

export type { Disposable, TershEvents };
export { Emitter } from "../../../app/events";

export interface GridInfo {
  cols: number;
  rows: number;
  /** CSS pixels. */
  cellWidth: number;
  cellHeight: number;
  cursor: { x: number; y: number };
  altScreen: boolean;
  /** Text of a visible row. */
  rowText(y: number): string;
}

/** A bound key is consumed unless its handler returns `false`. */
export type KeyHandler = AppKeyHandler<KeyboardEvent>;
/** A named command, bindable in `[keybindings]`. Return `false` to let the key through to the program. */
export type ActionHandler = AppActionHandler<KeyboardEvent>;
/** Return `true` to consume the message so the default handling is skipped. */
export type { RichHandler };

export interface TershAPI {
  readonly version: string;
  /** The current config; replaced (not mutated) on reload, so read it when needed. */
  readonly config: Readonly<Config>;
  /** Add a named action that users can bind in `[keybindings]` (e.g. `"myplugin.toggle"`). */
  registerAction(name: string, handler: ActionHandler): Disposable;
  /** Run a built-in or registered action. Returns false if it doesn't exist or declined. */
  runAction(name: string): boolean;
  /** Names of every action, built-in and registered. */
  actions(): string[];
  /** Render cards of `kind` (the `kind` field of rich messages). Replaces any existing renderer. */
  registerRenderer(kind: string, renderer: CardRenderer): Disposable;
  /** Bind a key combo such as `"ctrl+shift+k"` or `"alt+enter"`. Handlers run before terminal input. */
  registerKeybinding(combo: string, handler: KeyHandler): Disposable;
  /** See every rich message before it is handled; return `true` to consume it (e.g. to add new ops). */
  onRich(handler: RichHandler): Disposable;
  on<K extends keyof TershEvents>(event: K, handler: (payload: TershEvents[K]) => void): Disposable;
  /** Send bytes to the shell as if typed. */
  write(text: string): Promise<void>;
  /** Paste text, honouring bracketed paste mode. */
  paste(text: string): Promise<void>;
  cards: {
    show(message: RichMessage): string;
    update(message: RichMessage): void;
    close(id: string): void;
    clear(): void;
    list(): string[];
  };
  grid(): GridInfo;
  notify(message: string, level?: "info" | "error"): void;
  addStyle(css: string): Disposable;
  /** Profiling numbers: what the page measured, and the session's counters (`null` without a session). */
  profile(): Promise<{ ui: UiProfile; session: SessionStats | null }>;
}

/** The API for `app` drawn on `screen`. */
export function createApi(app: TerminalApp<KeyboardEvent>, screen: DomScreen, backend: Backend): TershAPI {
  const { cards } = screen;
  return {
    version: packageVersion,
    get config() {
      return app.currentConfig;
    },
    registerAction: (name, handler) => app.registerAction(name, handler),
    runAction: (name) => app.runAction(name),
    actions: () => app.actionNames(),
    registerRenderer(kind: string, renderer: CardRenderer) {
      const previous = cards.renderers.get(kind);
      cards.renderers.set(kind, renderer);
      cards.rerender(kind);
      return {
        dispose() {
          if (cards.renderers.get(kind) !== renderer) return;
          if (previous) cards.renderers.set(kind, previous);
          else cards.renderers.delete(kind);
        },
      };
    },
    registerKeybinding: (combo, handler) => app.registerKeybinding(combo, handler),
    onRich: (handler) => app.onRich(handler),
    on: (event, handler) => app.events.on(event, handler),
    write: (text) => app.write(text),
    paste: (text) => app.paste(text),
    cards: {
      show: (message) => cards.show(message),
      update: (message) => cards.update(message),
      close: (id) => cards.close(id),
      clear: () => cards.handle({ op: "clear" }),
      list: () => cards.ids,
    },
    grid(): GridInfo {
      const r = screen.renderer;
      return {
        cols: r.cols,
        rows: r.rows,
        cellWidth: r.metrics.cellWidth,
        cellHeight: r.metrics.cellHeight,
        cursor: { x: r.frame?.cursor.x ?? 0, y: r.frame?.cursor.y ?? 0 },
        altScreen: r.frame?.alt ?? false,
        rowText: (y) => r.rowText(y),
      };
    },
    notify: (message, level) => app.notify(message, level),
    async profile() {
      const session = await backend.stats().catch(() => null);
      return { ui: screen.profiler.snapshot(), session };
    },
    addStyle(css: string) {
      const style = document.createElement("style");
      style.textContent = css;
      document.head.append(style);
      return { dispose: () => style.remove() };
    },
  };
}
