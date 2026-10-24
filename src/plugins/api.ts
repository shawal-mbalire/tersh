// The API handed to plugins (and used by the built-in features).

import type { CardRenderer, RichMessage } from "../cards/manager";
import type { Config, Frame } from "../types";

export interface Disposable {
  dispose(): void;
}

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

/** A bound key is consumed unless its handler returns `false`. */
export type KeyHandler = (event: KeyboardEvent) => boolean | void;
/** A named command, bindable in `[keybindings]`. Return `false` to let the key through to the program. */
export type ActionHandler = (event?: KeyboardEvent) => boolean | void | Promise<void>;
/** Return `true` to consume the message so the default handling is skipped. */
export type RichHandler = (message: RichMessage) => boolean | void;

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
}

/** Normalise `"Shift+Ctrl+K"` → `"ctrl+shift+k"`. A literal plus key may be written `plus` or `+`. */
export function normalizeCombo(combo: string): string {
  const parts = combo
    .trim()
    .replace(/(^|\+)\+$/, "$1plus")
    .toLowerCase()
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean);
  const key = parts.pop() ?? "";
  const alias: Record<string, string> = {
    cmd: "meta",
    super: "meta",
    win: "meta",
    control: "ctrl",
    option: "alt",
    esc: "escape",
    space: " ",
    plus: "+",
  };
  const mods = new Set(parts.map((p) => alias[p] ?? p));
  const order = ["ctrl", "alt", "shift", "meta"].filter((m) => mods.has(m));
  return [...order, alias[key] ?? key].join("+");
}

/** The combo string for a keyboard event, comparable with `normalizeCombo` output. */
export function comboFromEvent(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string {
  const mods = [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "meta"].filter(Boolean);
  return [...mods, e.key.toLowerCase()].join("+");
}

export class Emitter {
  private handlers = new Map<string, Set<(payload: never) => void>>();

  on<K extends keyof TershEvents>(event: K, handler: (payload: TershEvents[K]) => void): Disposable {
    let set = this.handlers.get(event);
    if (!set) this.handlers.set(event, (set = new Set()));
    set.add(handler as (payload: never) => void);
    return { dispose: () => set!.delete(handler as (payload: never) => void) };
  }

  emit<K extends keyof TershEvents>(event: K, payload: TershEvents[K]) {
    for (const handler of this.handlers.get(event) ?? []) {
      try {
        (handler as (payload: TershEvents[K]) => void)(payload);
      } catch (e) {
        console.error(`tersh: ${event} handler failed`, e);
      }
    }
  }
}
