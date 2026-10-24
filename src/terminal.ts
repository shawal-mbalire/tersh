// Wires the canvas grid, input, selection and card overlay to the Rust session.

import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { CardManager, type CardRenderer, type RichMessage } from "./cards/manager";
import { builtinRenderers } from "./cards/renderers";
import {
  type ActionHandler,
  comboFromEvent,
  Emitter,
  type GridInfo,
  type KeyHandler,
  normalizeCombo,
  type RichHandler,
  type TershAPI,
} from "./plugins/api";
import { resolveKeybindings } from "./settings";
import { encodeKey, encodeMouse, encodePaste, type MouseAction } from "./term/input";
import { Palette } from "./term/palette";
import { GridRenderer } from "./term/renderer";
import type { Binding, Config, Frame, SessionInfo, TermEvent } from "./types";
import { Mode } from "./types";

interface Cell {
  x: number;
  y: number;
}

const isMac = navigator.userAgent.includes("Mac");

export class Terminal {
  readonly events = new Emitter();
  readonly cards: CardManager;
  readonly api: TershAPI;

  private renderer: GridRenderer;
  private keybindings = new Map<string, KeyHandler[]>();
  /** `[keybindings]` from the config merged over the defaults. */
  private bindings: Map<string, Binding>;
  private actions = new Map<string, ActionHandler>();
  private richHandlers = new Set<RichHandler>();
  private paintPending = false;
  private resizeTimer = 0;
  private composing = false;
  private mouseButton: number | null = null;
  private wheelRemainder = 0;
  private selection: { anchor: Cell; head: Cell } | null = null;
  private selecting = false;
  private fontSize: number;
  private started = false;

  constructor(
    private config: Config,
    private readonly els: {
      terminal: HTMLElement;
      canvas: HTMLCanvasElement;
      cursor: HTMLElement;
      selection: HTMLElement;
      cards: HTMLElement;
      input: HTMLTextAreaElement;
      toasts: HTMLElement;
    },
  ) {
    this.fontSize = config.font.size;
    this.renderer = new GridRenderer(els.canvas, els.cursor, config.font, new Palette(config.theme, config.font.bold_is_bright));
    this.cards = new CardManager(els.cards, (message) => this.notify(message, "error"));
    for (const [kind, renderer] of Object.entries(builtinRenderers)) this.cards.renderers.set(kind, renderer);
    this.bindings = resolveKeybindings(config.keybindings, isMac);
    this.registerBuiltinActions();
    this.api = this.createApi();
    this.bindInput();
    this.bindMouse();
  }

  private get mode(): number {
    return this.renderer.frame?.mode ?? 0;
  }

  async start() {
    const { cols, rows } = this.measureGrid();
    this.renderer.resize(cols, rows);

    const frames = new Channel<Frame>();
    frames.onmessage = (frame) => this.onFrame(frame);
    const rich = new Channel<RichMessage>();
    rich.onmessage = (message) => this.onRich(message);
    const events = new Channel<TermEvent>();
    events.onmessage = (event) => this.onEvent(event);

    await invoke<SessionInfo>("session_start", { cols, rows, frames, rich, events });
    this.started = true;
    this.sendResize(cols, rows);
    new ResizeObserver(() => this.scheduleResize()).observe(this.els.terminal);
    this.els.input.focus();
  }

  write(text: string): Promise<void> {
    this.clearSelection();
    return invoke("session_write", { data: text });
  }

  paste(text: string): Promise<void> {
    return this.write(encodePaste(text, this.mode));
  }

  /** Apply a reloaded config: colours, font, cursor and keybindings change in place. */
  reconfigure(config: Config) {
    const previous = this.config;
    this.config = config;
    this.bindings = resolveKeybindings(config.keybindings, isMac);
    this.renderer.palette = new Palette(config.theme, config.font.bold_is_bright);
    const font = config.font;
    const old = previous.font;
    if (font.family !== old.family || font.size !== old.size || font.line_height !== old.line_height) {
      // A new font size in the file wins over the current zoom.
      if (font.size !== old.size) this.fontSize = font.size;
      this.applyFont();
    } else {
      this.renderer.invalidate();
      this.schedulePaint();
    }
    this.events.emit("config", config);
  }

  /** Run a built-in or plugin action by name. */
  runAction(name: string, event?: KeyboardEvent): boolean {
    const action = this.actions.get(name);
    if (!action) {
      this.notify(`unknown action "${name}" (see config.toml for the list)`, "error");
      return false;
    }
    try {
      const result = action(event);
      if (result instanceof Promise) {
        result.catch((e) => this.notify(`${name}: ${e instanceof Error ? e.message : String(e)}`, "error"));
        return true;
      }
      return result !== false;
    } catch (e) {
      this.notify(`${name}: ${e instanceof Error ? e.message : String(e)}`, "error");
      return true;
    }
  }

  private registerBuiltinActions() {
    const notifyError = (e: unknown) => this.notify(e instanceof Error ? e.message : String(e), "error");
    const page = () => Math.max(1, this.renderer.rows - 1);
    // History scrolling only makes sense on the normal screen; full-screen apps get the key.
    const onPrimary = (f: () => void) => () => {
      if (this.mode & Mode.ALT_SCREEN) return false;
      f();
    };
    const builtins: Record<string, ActionHandler> = {
      copy: () => {
        const text = this.selectionText();
        if (text) void writeText(text);
      },
      paste: () => {
        void readText().then((text) => {
          if (text) void this.paste(text);
        });
      },
      zoom_in: () => this.setFontSize(this.fontSize + 1),
      zoom_out: () => this.setFontSize(this.fontSize - 1),
      zoom_reset: () => this.setFontSize(this.config.font.size),
      scroll_page_up: onPrimary(() => this.scrollHistory(page())),
      scroll_page_down: onPrimary(() => this.scrollHistory(-page())),
      scroll_top: onPrimary(() => this.scrollHistory(this.config.scrollback)),
      scroll_bottom: () => void invoke("session_scroll_to_bottom"),
      clear_cards: () => {
        this.cards.handle({ op: "clear" });
      },
      reload_config: () => void invoke("reload_config").catch(notifyError),
      open_config: () => void invoke("open_config").catch(notifyError),
    };
    for (const [name, handler] of Object.entries(builtins)) this.actions.set(name, handler);
  }

  notify(message: string, level: "info" | "error" = "info") {
    const toast = document.createElement("div");
    toast.className = `toast ${level}`;
    toast.textContent = message;
    this.els.toasts.append(toast);
    setTimeout(() => toast.remove(), level === "error" ? 8000 : 4000);
  }

  // ---- session output ------------------------------------------------------

  private onFrame(frame: Frame) {
    const scrolled = this.renderer.frame && this.renderer.frame.display_offset !== frame.display_offset;
    this.renderer.apply(frame);
    if (scrolled) this.clearSelection();
    this.events.emit("frame", frame);
    this.schedulePaint();
  }

  private schedulePaint() {
    if (this.paintPending) return;
    this.paintPending = true;
    requestAnimationFrame(() => {
      this.paintPending = false;
      this.renderer.draw();
      const frame = this.renderer.frame;
      if (frame) {
        this.cards.layout({ frame, metrics: this.renderer.metrics });
        this.placeInput(frame);
      }
      this.drawSelection();
    });
  }

  private onRich(message: RichMessage) {
    for (const handler of this.richHandlers) {
      try {
        if (handler(message) === true) return;
      } catch (e) {
        console.error(e);
      }
    }
    this.cards.handle(message);
  }

  private onEvent(event: TermEvent) {
    switch (event.type) {
      case "title":
        void getCurrentWindow().setTitle(event.title || "tersh");
        this.events.emit("title", event.title);
        break;
      case "reset_title":
        void getCurrentWindow().setTitle("tersh");
        this.events.emit("title", "tersh");
        break;
      case "bell":
        this.els.terminal.classList.remove("bell");
        void this.els.terminal.offsetWidth;
        this.els.terminal.classList.add("bell");
        this.events.emit("bell", undefined);
        break;
      case "clipboard":
        void writeText(event.text);
        break;
      case "rich_error":
        this.notify(`rich message rejected: ${event.error}`, "error");
        break;
      case "exit":
        this.events.emit("exit", event.code);
        void getCurrentWindow().close();
        break;
    }
  }

  // ---- geometry ------------------------------------------------------------

  private measureGrid() {
    const { clientWidth, clientHeight } = this.els.terminal;
    return this.renderer.fit(clientWidth, clientHeight);
  }

  private scheduleResize() {
    clearTimeout(this.resizeTimer);
    this.resizeTimer = window.setTimeout(() => {
      const { cols, rows } = this.measureGrid();
      if (cols === this.renderer.cols && rows === this.renderer.rows) return;
      this.renderer.resize(cols, rows);
      this.sendResize(cols, rows);
    }, 16);
  }

  private sendResize(cols: number, rows: number) {
    if (!this.started) return;
    const { cellWidth, cellHeight } = this.renderer.metrics;
    void invoke("session_resize", { cols, rows, cellWidth: Math.round(cellWidth), cellHeight: Math.round(cellHeight) });
    this.events.emit("resize", { cols, rows });
  }

  private setFontSize(size: number) {
    this.fontSize = Math.min(72, Math.max(6, size));
    this.applyFont();
  }

  private applyFont() {
    this.renderer.setFont({ ...this.config.font, size: this.fontSize });
    this.els.cursor.style.fontSize = `${this.fontSize}px`;
    const { cols, rows } = this.measureGrid();
    this.renderer.resize(cols, rows);
    this.sendResize(cols, rows);
    this.schedulePaint();
  }

  /** Keep the hidden textarea at the cursor so IME candidate windows appear there. */
  private placeInput(frame: Frame) {
    const { cellWidth, cellHeight } = this.renderer.metrics;
    this.els.input.style.transform = `translate(${frame.cursor.x * cellWidth}px, ${frame.cursor.y * cellHeight}px)`;
    this.els.input.style.height = `${cellHeight}px`;
  }

  private cellFromEvent(e: MouseEvent): Cell {
    const rect = this.els.canvas.getBoundingClientRect();
    const { cellWidth, cellHeight } = this.renderer.metrics;
    return {
      x: Math.min(this.renderer.cols - 1, Math.max(0, Math.floor((e.clientX - rect.left) / cellWidth))),
      y: Math.min(this.renderer.rows - 1, Math.max(0, Math.floor((e.clientY - rect.top) / cellHeight))),
    };
  }

  // ---- keyboard ------------------------------------------------------------

  private bindInput() {
    const input = this.els.input;

    input.addEventListener("keydown", (e) => {
      if (e.isComposing || this.composing || e.keyCode === 229) return;
      const handlers = this.keybindings.get(comboFromEvent(e));
      if (handlers?.length) {
        for (const handler of [...handlers].reverse()) {
          if (handler(e) !== false) {
            e.preventDefault();
            return;
          }
        }
      }
      const binding = this.bindings.get(comboFromEvent(e));
      if (binding !== undefined) {
        const handled = typeof binding === "string" ? this.runAction(binding, e) : (void this.write(binding.send), true);
        if (handled) {
          e.preventDefault();
          return;
        }
      }
      const data = encodeKey(e, this.mode);
      if (data !== null) {
        e.preventDefault();
        void this.write(data);
      }
    });

    input.addEventListener("input", () => {
      if (this.composing) return;
      const text = input.value;
      input.value = "";
      if (text) void this.write(text);
    });
    input.addEventListener("compositionstart", () => (this.composing = true));
    input.addEventListener("compositionend", (e) => {
      this.composing = false;
      input.value = "";
      if (e.data) void this.write(e.data);
    });
    input.addEventListener("paste", (e) => {
      e.preventDefault();
      const text = e.clipboardData?.getData("text/plain");
      if (text) void this.paste(text);
    });

    const focusChanged = (focused: boolean) => {
      this.els.terminal.classList.toggle("unfocused", !focused);
      if (this.mode & Mode.FOCUS_IN_OUT) void invoke("session_write", { data: focused ? "\x1b[I" : "\x1b[O" });
      this.events.emit("focus", focused);
    };
    window.addEventListener("focus", () => {
      focusChanged(true);
      if (!document.activeElement || document.activeElement === document.body) input.focus();
    });
    window.addEventListener("blur", () => focusChanged(false));
  }

  private scrollHistory(lines: number) {
    void invoke("session_scroll", { lines });
  }

  // ---- mouse ---------------------------------------------------------------

  private reportsMouse(e: MouseEvent): boolean {
    return (this.mode & Mode.MOUSE_MODE) !== 0 && !e.shiftKey;
  }

  private sendMouse(action: MouseAction, e: MouseEvent, button = 0) {
    const { x, y } = this.cellFromEvent(e);
    const data = encodeMouse(
      { action, button, held: this.mouseButton !== null, col: x, row: y, shift: e.shiftKey, alt: e.altKey, ctrl: e.ctrlKey },
      this.mode,
    );
    if (data) void invoke("session_write", { data });
  }

  private bindMouse() {
    const surface = this.els.terminal;
    const onGrid = (e: Event) => !(e.target as HTMLElement).closest(".card");

    surface.addEventListener("mousedown", (e) => {
      if (!onGrid(e)) return;
      e.preventDefault();
      this.els.input.focus();
      if (this.reportsMouse(e)) {
        this.mouseButton = e.button;
        this.sendMouse("press", e, e.button);
        return;
      }
      if (e.button === 1) {
        void readText().then((text) => {
          if (text) void this.paste(text);
        });
        return;
      }
      if (e.button !== 0) return;
      const cell = this.cellFromEvent(e);
      if (e.detail === 2) {
        this.selectWord(cell);
        return;
      }
      if (e.detail >= 3) {
        this.selection = { anchor: { x: 0, y: cell.y }, head: { x: this.renderer.cols - 1, y: cell.y } };
        this.drawSelection();
        return;
      }
      this.selection = { anchor: cell, head: cell };
      this.selecting = true;
      this.drawSelection();
    });

    window.addEventListener("mousemove", (e) => {
      if (this.selecting && this.selection) {
        this.selection.head = this.cellFromEvent(e);
        this.drawSelection();
      } else if (this.reportsMouse(e) && onGrid(e)) {
        this.sendMouse("move", e, this.mouseButton ?? 0);
      }
    });

    window.addEventListener("mouseup", (e) => {
      if (this.mouseButton !== null) {
        const button = this.mouseButton;
        this.mouseButton = null;
        this.sendMouse("release", e, button);
      }
      if (this.selecting) {
        this.selecting = false;
        const s = this.selection;
        if (s && s.anchor.x === s.head.x && s.anchor.y === s.head.y) this.clearSelection();
      }
    });

    surface.addEventListener(
      "wheel",
      (e) => {
        if (!onGrid(e)) return;
        e.preventDefault();
        const { cellHeight } = this.renderer.metrics;
        const delta =
          e.deltaMode === WheelEvent.DOM_DELTA_LINE
            ? e.deltaY
            : e.deltaMode === WheelEvent.DOM_DELTA_PAGE
              ? e.deltaY * this.renderer.rows
              : e.deltaY / cellHeight;
        this.wheelRemainder += delta;
        const lines = Math.trunc(this.wheelRemainder);
        if (lines === 0) return;
        this.wheelRemainder -= lines;

        if (this.reportsMouse(e)) {
          for (let i = 0; i < Math.abs(lines); i++) this.sendMouse(lines < 0 ? "wheel-up" : "wheel-down", e);
        } else if (this.mode & Mode.ALT_SCREEN && this.mode & Mode.ALTERNATE_SCROLL) {
          const key = lines < 0 ? "A" : "B";
          const prefix = this.mode & Mode.APP_CURSOR ? "\x1bO" : "\x1b[";
          void invoke("session_write", { data: (prefix + key).repeat(Math.abs(lines)) });
        } else {
          this.scrollHistory(-lines);
        }
      },
      { passive: false },
    );
  }

  // ---- selection -----------------------------------------------------------

  private selectWord(cell: Cell) {
    const text = [...this.renderer.rowText(cell.y)];
    const isWord = (ch: string | undefined) => !!ch && !/[\s"'`()[\]{}<>|;,]/.test(ch);
    if (!isWord(text[cell.x])) return;
    let start = cell.x;
    let end = cell.x;
    while (start > 0 && isWord(text[start - 1])) start--;
    while (end < text.length - 1 && isWord(text[end + 1])) end++;
    this.selection = { anchor: { x: start, y: cell.y }, head: { x: end, y: cell.y } };
    this.drawSelection();
  }

  private orderedSelection(): [Cell, Cell] | null {
    const s = this.selection;
    if (!s) return null;
    const before = s.anchor.y < s.head.y || (s.anchor.y === s.head.y && s.anchor.x <= s.head.x);
    return before ? [s.anchor, s.head] : [s.head, s.anchor];
  }

  selectionText(): string {
    const range = this.orderedSelection();
    if (!range) return "";
    const [start, end] = range;
    const lines: string[] = [];
    for (let y = start.y; y <= end.y; y++) {
      const from = y === start.y ? start.x : 0;
      const to = y === end.y ? end.x + 1 : this.renderer.cols;
      lines.push(this.renderer.rowText(y, from, to).trimEnd());
    }
    return lines.join("\n");
  }

  private clearSelection() {
    if (!this.selection) return;
    this.selection = null;
    this.selecting = false;
    this.drawSelection();
  }

  private drawSelection() {
    const layer = this.els.selection;
    const range = this.orderedSelection();
    if (!range) {
      layer.replaceChildren();
      return;
    }
    const [start, end] = range;
    const { cellWidth: cw, cellHeight: ch } = this.renderer.metrics;
    const boxes: HTMLElement[] = [];
    for (let y = start.y; y <= end.y; y++) {
      const from = y === start.y ? start.x : 0;
      const to = y === end.y ? end.x + 1 : this.renderer.cols;
      const box = document.createElement("div");
      box.style.transform = `translate(${from * cw}px, ${y * ch}px)`;
      box.style.width = `${(to - from) * cw}px`;
      box.style.height = `${ch}px`;
      boxes.push(box);
    }
    layer.replaceChildren(...boxes);
  }

  // ---- plugin API ----------------------------------------------------------

  private createApi(): TershAPI {
    const terminal = this;
    return {
      version: "0.1.0",
      get config() {
        return terminal.config;
      },
      registerAction(name: string, handler: ActionHandler) {
        terminal.actions.set(name, handler);
        return {
          dispose() {
            if (terminal.actions.get(name) === handler) terminal.actions.delete(name);
          },
        };
      },
      runAction: (name) => terminal.runAction(name),
      actions: () => [...terminal.actions.keys()].sort(),
      registerRenderer(kind: string, renderer: CardRenderer) {
        const previous = terminal.cards.renderers.get(kind);
        terminal.cards.renderers.set(kind, renderer);
        terminal.cards.rerender(kind);
        return {
          dispose() {
            if (terminal.cards.renderers.get(kind) !== renderer) return;
            if (previous) terminal.cards.renderers.set(kind, previous);
            else terminal.cards.renderers.delete(kind);
          },
        };
      },
      registerKeybinding(combo: string, handler: KeyHandler) {
        const key = normalizeCombo(combo);
        const list = terminal.keybindings.get(key) ?? [];
        list.push(handler);
        terminal.keybindings.set(key, list);
        return { dispose: () => list.splice(list.indexOf(handler) >>> 0, 1) };
      },
      onRich(handler: RichHandler) {
        terminal.richHandlers.add(handler);
        return { dispose: () => terminal.richHandlers.delete(handler) };
      },
      on: (event, handler) => terminal.events.on(event, handler),
      write: (text) => terminal.write(text),
      paste: (text) => terminal.paste(text),
      cards: {
        show: (message) => terminal.cards.show(message),
        update: (message) => terminal.cards.update(message),
        close: (id) => terminal.cards.close(id),
        clear: () => terminal.cards.handle({ op: "clear" }),
        list: () => terminal.cards.ids,
      },
      grid(): GridInfo {
        const r = terminal.renderer;
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
      notify: (message, level) => terminal.notify(message, level),
      addStyle(css: string) {
        const style = document.createElement("style");
        style.textContent = css;
        document.head.append(style);
        return { dispose: () => style.remove() };
      },
    };
  }
}
