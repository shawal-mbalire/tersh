// The terminal use case: what keys, mouse reports, actions and session events
// mean. It talks to the session through the Backend port and draws through
// the Screen port; the page (adapters/dom) turns DOM events into calls here.

import type { RichMessage } from "@domain/cards";
import type { Disposable } from "@domain/events";
import type { Cell } from "@domain/selection";
import { comboFromEvent, normalizeCombo, resolveKeybindings } from "@domain/keys";
import { encodeKey, encodeMouse, encodePaste, type KeyLike, type MouseAction, normalizeTyped } from "@domain/input";
import { Palette } from "@domain/palette";
import { wheelAction, wheelKeys } from "@domain/wheel";
import { type Binding, type Config, type Frame, Mode, type TermEvent } from "@domain/types";
import type { Backend } from "@ports/backend";
import type { NotifyLevel, Screen } from "@ports/screen";
import { Emitter } from "./events";

/** Font sizes zooming stays within. */
export const MIN_FONT_SIZE = 6;
export const MAX_FONT_SIZE = 72;
/** How long to wait for a first paint before letting the host show the window anyway. */
export const READY_FALLBACK_MS = 300;

/** A bound key is consumed unless its handler returns `false`. */
export type KeyHandler<K> = (event: K) => boolean | void;
/** A named command, bindable in `[keybindings]`. Return `false` to let the key through to the program. */
export type ActionHandler<K> = (event?: K) => boolean | void | Promise<void>;
/** Return `true` to consume the message so the default handling is skipped. */
export type RichHandler = (message: RichMessage) => boolean | void;

export interface Modifiers {
  shift: boolean;
  alt: boolean;
  ctrl: boolean;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export class TerminalApp<K extends KeyLike = KeyLike> {
  readonly events = new Emitter();

  private bindings: Map<string, Binding>;
  private keybindings = new Map<string, KeyHandler<K>[]>();
  private actions = new Map<string, ActionHandler<K>>();
  private richHandlers = new Set<RichHandler>();
  private fontSize: number;
  /** The latest frame's metadata (mode, cursor, scroll state). */
  private frame: Frame | null = null;
  private started = false;
  /** Whether the host was told the first frame is on screen. */
  private ready = false;
  /** A frame arrived that hasn't been painted (and acknowledged) yet. */
  private unpainted = false;
  /** Whether the last key pressed was the space bar (see `normalizeTyped`). */
  private spaceKey = false;
  /** Mouse button held while reporting to the program. */
  private heldButton: number | null = null;

  constructor(
    private config: Config,
    private readonly backend: Backend,
    private readonly screen: Screen,
    private readonly mac: boolean,
  ) {
    this.fontSize = config.font.size;
    this.bindings = resolveKeybindings(config.keybindings, mac);
    this.registerBuiltinActions();
  }

  get currentConfig(): Config {
    return this.config;
  }

  get mode(): number {
    return this.frame?.mode ?? 0;
  }

  async start() {
    const { cols, rows } = this.screen.fit();
    this.screen.resize(cols, rows);
    const { cellWidth, cellHeight } = this.screen.metrics;
    await this.backend.startSession(cols, rows, Math.round(cellWidth), Math.round(cellHeight), {
      frame: (frame) => this.receiveFrame(frame),
      rich: (message) => this.receiveRich(message),
      event: (event) => this.receiveEvent(event),
    });
    this.started = true;
    // A hidden window may not run animation frames, so don't wait on a paint forever.
    setTimeout(() => this.windowReady(), READY_FALLBACK_MS);
  }

  /** Let the host show the window (it starts hidden so it never flashes empty). */
  windowReady() {
    if (this.ready) return;
    this.ready = true;
    void this.backend.windowReady().catch(() => undefined);
  }

  /** The screen was painted: tell the session the frames received so far are
   *  on screen, so it sends the next one. */
  painted() {
    if (this.frame) this.windowReady();
    if (!this.unpainted) return;
    this.unpainted = false;
    void this.backend.frameShown().catch(() => undefined);
  }

  write(text: string): Promise<void> {
    this.screen.clearSelection();
    return this.backend.write(text);
  }

  paste(text: string): Promise<void> {
    if (this.backend.paste) {
      this.screen.clearSelection();
      return this.backend.paste(text);
    }
    return this.write(encodePaste(text, this.mode));
  }

  pasteClipboard() {
    void this.backend.readClipboard().then((text) => {
      if (text) void this.paste(text);
    });
  }

  notify(text: string, level: NotifyLevel = "info") {
    this.screen.notify(text, level);
  }

  /** Apply a reloaded config: colours, font, cursor and keybindings change in place. */
  reconfigure(config: Config) {
    const previous = this.config;
    this.config = config;
    this.bindings = resolveKeybindings(config.keybindings, this.mac);
    this.screen.restyle(new Palette(config.theme, config.font.bold_is_bright), config.window.opacity);
    const font = config.font;
    const old = previous.font;
    if (font.family !== old.family || font.size !== old.size || font.line_height !== old.line_height) {
      // A new font size in the file wins over the current zoom.
      if (font.size !== old.size) this.fontSize = font.size;
      this.applyFont();
    } else {
      this.screen.repaint();
    }
    this.events.emit("config", config);
  }

  /** Repaint the whole screen, e.g. after the host remapped the window. */
  redraw() {
    this.screen.repaint();
  }

  /** Re-measure the grid, e.g. after the installed fonts changed. */
  refreshFonts() {
    this.applyFont();
  }

  /** The visible area changed size. */
  resized() {
    const { cols, rows } = this.screen.fit();
    if (cols === this.screen.cols && rows === this.screen.rows) return;
    this.screen.resize(cols, rows);
    this.sendResize(cols, rows);
  }

  // ---- actions and keys ------------------------------------------------------

  /** Run a built-in or plugin action by name. */
  runAction(name: string, event?: K): boolean {
    const action = this.actions.get(name);
    if (!action) {
      this.notify(`unknown action "${name}" (see config.toml for the list)`, "error");
      return false;
    }
    try {
      const result = action(event);
      if (result instanceof Promise) {
        result.catch((e) => this.notify(`${name}: ${message(e)}`, "error"));
        return true;
      }
      return result !== false;
    } catch (e) {
      this.notify(`${name}: ${message(e)}`, "error");
      return true;
    }
  }

  registerAction(name: string, handler: ActionHandler<K>): Disposable {
    this.actions.set(name, handler);
    return {
      dispose: () => {
        if (this.actions.get(name) === handler) this.actions.delete(name);
      },
    };
  }

  actionNames(): string[] {
    return [...this.actions.keys()].sort();
  }

  /** Bind a combo such as `"ctrl+shift+k"`; handlers run before terminal input, newest first. */
  registerKeybinding(combo: string, handler: KeyHandler<K>): Disposable {
    const key = normalizeCombo(combo);
    const list = this.keybindings.get(key) ?? [];
    list.push(handler);
    this.keybindings.set(key, list);
    return { dispose: () => void list.splice(list.indexOf(handler) >>> 0, 1) };
  }

  onRich(handler: RichHandler): Disposable {
    this.richHandlers.add(handler);
    return { dispose: () => void this.richHandlers.delete(handler) };
  }

  /** A key was pressed. Returns whether it was used (so the page should swallow it). */
  key(event: K): boolean {
    this.spaceKey = event.key === " ";
    const combo = comboFromEvent(event);
    for (const handler of [...(this.keybindings.get(combo) ?? [])].reverse()) {
      if (handler(event) !== false) return true;
    }
    const binding = this.bindings.get(combo);
    if (binding !== undefined) {
      const handled = typeof binding === "string" ? this.runAction(binding, event) : (void this.write(binding.send), true);
      if (handled) return true;
    }
    const data = encodeKey(event, this.mode);
    if (data === null) return false;
    if (this.backend.key) {
      this.screen.clearSelection();
      void this.backend.key(event);
      return true;
    }
    void this.write(data);
    return true;
  }

  /** A key was released. Sent only when the program enabled Kitty event reporting. */
  keyUp(event: K): boolean {
    if (!(this.mode & Mode.REPORT_EVENT_TYPES)) return false;
    const upEvent = { ...event, type: "keyup" as const };
    const data = encodeKey(upEvent, this.mode);
    if (data === null) return false;
    if (this.backend.key) {
      this.screen.clearSelection();
      void this.backend.key(upEvent);
      return true;
    }
    void this.write(data);
    return true;
  }

  /** Text typed into the input field (not handled as a key). */
  typed(text: string) {
    const normalized = normalizeTyped(text, this.spaceKey);
    if (normalized) void this.write(normalized);
  }

  /** An input method finished composing `text`. */
  composed(text: string) {
    if (text) void this.write(text);
  }

  focusChanged(focused: boolean) {
    if (this.backend.focus) {
      void this.backend.focus(focused);
    } else if (this.mode & Mode.FOCUS_IN_OUT) {
      void this.backend.write(focused ? "\x1b[I" : "\x1b[O");
    }
    this.events.emit("focus", focused);
  }

  // ---- mouse ---------------------------------------------------------------

  /** Whether mouse events go to the program (it asked, and shift isn't held to select instead). */
  reportsMouse(shift: boolean): boolean {
    return (this.mode & Mode.MOUSE_MODE) !== 0 && !shift;
  }

  mouse(action: MouseAction, cell: Cell, button: number, mods: Modifiers) {
    if (action === "press") this.heldButton = button;
    if (action === "release") this.heldButton = null;
    const report = { action, button, held: this.heldButton !== null, col: cell.x, row: cell.y, ...mods };
    if (this.backend.mouse) {
      void this.backend.mouse(report);
      return;
    }
    const data = encodeMouse(report, this.mode);
    if (data) void this.backend.write(data);
  }

  /** The button held while reporting, to release it wherever the mouse is let go. */
  get mouseButton(): number | null {
    return this.heldButton;
  }

  /** The wheel moved `lines` (positive is down) over `cell`. */
  wheel(lines: number, cell: Cell, mods: Modifiers) {
    if (this.backend.wheel) {
      void this.backend.wheel(lines, cell.x, cell.y, mods.shift, mods.alt, mods.ctrl);
      return;
    }
    switch (wheelAction(this.mode, this.reportsMouse(mods.shift))) {
      case "report":
        for (let i = 0; i < Math.abs(lines); i++) this.mouse(lines < 0 ? "wheel-up" : "wheel-down", cell, 0, mods);
        break;
      case "keys":
        void this.backend.write(wheelKeys(lines, this.mode));
        break;
      case "history":
        void this.backend.scroll(-lines);
        break;
    }
  }

  // ---- session output --------------------------------------------------------

  private receiveFrame(frame: Frame) {
    this.frame = frame;
    this.unpainted = true;
    this.screen.show(frame);
    this.events.emit("frame", frame);
  }

  private receiveRich(message: RichMessage) {
    for (const handler of this.richHandlers) {
      try {
        if (handler(message) === true) return;
      } catch (e) {
        console.error(e);
      }
    }
    this.screen.rich(message);
  }

  private receiveEvent(event: TermEvent) {
    switch (event.type) {
      case "title":
        void this.backend.setTitle(event.title || "tersh");
        this.events.emit("title", event.title);
        break;
      case "reset_title":
        void this.backend.setTitle("tersh");
        this.events.emit("title", "tersh");
        break;
      case "bell":
        this.screen.bell();
        this.events.emit("bell", undefined);
        break;
      case "clipboard":
        void this.backend.writeClipboard(event.text);
        break;
      case "rich_error":
        this.notify(`rich message rejected: ${event.error}`, "error");
        break;
      case "exit":
        // The backend closes the window itself once the program has exited.
        this.events.emit("exit", event.code);
        break;
    }
  }

  // ---- internals -------------------------------------------------------------

  private registerBuiltinActions() {
    const notifyError = (e: unknown) => this.notify(message(e), "error");
    const page = () => Math.max(1, this.screen.rows - 1);
    // History scrolling only makes sense on the normal screen; full-screen apps get the key.
    const onPrimary = (f: () => void) => () => {
      if (this.mode & Mode.ALT_SCREEN) return false;
      f();
    };
    const builtins: Record<string, ActionHandler<K>> = {
      copy: () => {
        const text = this.screen.selectionText();
        if (text) void this.backend.writeClipboard(text);
      },
      paste: () => this.pasteClipboard(),
      zoom_in: () => this.setFontSize(this.fontSize + 1),
      zoom_out: () => this.setFontSize(this.fontSize - 1),
      zoom_reset: () => this.setFontSize(this.config.font.size),
      scroll_page_up: onPrimary(() => void this.backend.scroll(page())),
      scroll_page_down: onPrimary(() => void this.backend.scroll(-page())),
      scroll_top: onPrimary(() => void this.backend.scroll(this.config.scrollback)),
      scroll_bottom: () => void this.backend.scrollToBottom(),
      clear_cards: () => this.screen.rich({ op: "clear" }),
      reload_config: () => void this.backend.reloadConfig().catch(notifyError),
      open_config: () => void this.backend.openConfig().catch(notifyError),
      restart: () => void this.backend.restart().catch(notifyError),
    };
    for (const [name, handler] of Object.entries(builtins)) this.actions.set(name, handler);
  }

  private sendResize(cols: number, rows: number) {
    if (!this.started) return;
    const { cellWidth, cellHeight } = this.screen.metrics;
    void this.backend.resize(cols, rows, Math.round(cellWidth), Math.round(cellHeight));
    this.events.emit("resize", { cols, rows });
  }

  private setFontSize(size: number) {
    this.fontSize = Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, size));
    this.applyFont();
  }

  private applyFont() {
    const prev = this.screen.metrics;
    const prevCols = this.screen.cols;
    const prevRows = this.screen.rows;
    this.screen.setFont({ ...this.config.font, size: this.fontSize });
    const { cols, rows } = this.screen.fit();
    const next = this.screen.metrics;
    if (cols !== prevCols || rows !== prevRows || next.cellWidth !== prev.cellWidth || next.cellHeight !== prev.cellHeight) {
      this.screen.resize(cols, rows);
      this.sendResize(cols, rows);
    }
    this.screen.repaint();
  }
}
