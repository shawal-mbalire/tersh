// The terminal UI: wires the canvas grid, input, selection and card overlay to
// a session through the Backend port.

import { version as packageVersion } from "../../package.json";
import { CardManager, type CardRenderer, type RichMessage } from "./cards/manager";
import { builtinRenderers } from "@tersh/cards";
import { comboFromEvent, normalizeCombo, resolveKeybindings } from "../domain/keys";
import { type ActionHandler, Emitter, type GridInfo, type KeyHandler, type RichHandler, type TershAPI } from "./plugins/api";
import { encodeKey, encodeMouse, encodePaste, type MouseAction, normalizeTyped } from "../domain/input";
import { Palette } from "../domain/palette";
import { GridRenderer } from "./renderer";
import type { Binding, Config, Frame, TermEvent } from "../domain/types";
import type { Backend } from "../ports/backend";
import { Mode } from "../domain/types";

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
  /** Whether the last key pressed was the space bar (see `normalizeTyped`). */
  private spaceKey = false;
  private mouseButton: number | null = null;
  private wheelRemainder = 0;
  private selection: { anchor: Cell; head: Cell } | null = null;
  private selecting = false;
  private fontSize: number;
  private started = false;

  constructor(
    private config: Config,
    private readonly backend: Backend,
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
    this.renderer = new GridRenderer(
      els.canvas,
      els.cursor,
      config.font,
      new Palette(config.theme, config.font.bold_is_bright),
      config.window.opacity,
    );
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
    const { cellWidth, cellHeight } = this.renderer.metrics;

    await this.backend.startSession(cols, rows, Math.round(cellWidth), Math.round(cellHeight), {
      frame: (frame) => this.onFrame(frame),
      rich: (message) => this.onRich(message),
      event: (event) => this.onEvent(event),
    });
    this.started = true;
    new ResizeObserver(() => this.scheduleResize()).observe(this.els.terminal);
    this.els.input.focus();
  }

  write(text: string): Promise<void> {
    this.clearSelection();
    return this.backend.write(text);
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
    this.renderer.opacity = config.window.opacity;
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

  /** Re-measure the grid, e.g. after the installed fonts changed. */
  refreshFonts() {
    this.applyFont();
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
        if (text) void this.backend.writeClipboard(text);
      },
      paste: () => {
        void this.backend.readClipboard().then((text) => {
          if (text) void this.paste(text);
        });
      },
      zoom_in: () => this.setFontSize(this.fontSize + 1),
      zoom_out: () => this.setFontSize(this.fontSize - 1),
      zoom_reset: () => this.setFontSize(this.config.font.size),
      scroll_page_up: onPrimary(() => this.scrollHistory(page())),
      scroll_page_down: onPrimary(() => this.scrollHistory(-page())),
      scroll_top: onPrimary(() => this.scrollHistory(this.config.scrollback)),
      scroll_bottom: () => void this.backend.scrollToBottom(),
      clear_cards: () => {
        this.cards.handle({ op: "clear" });
      },
      reload_config: () => void this.backend.reloadConfig().catch(notifyError),
      open_config: () => void this.backend.openConfig().catch(notifyError),
      restart: () => void this.backend.restart().catch(notifyError),
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
        void this.backend.setTitle(event.title || "tersh");
        this.events.emit("title", event.title);
        break;
      case "reset_title":
        void this.backend.setTitle("tersh");
        this.events.emit("title", "tersh");
        break;
      case "bell":
        this.els.terminal.classList.remove("bell");
        void this.els.terminal.offsetWidth;
        this.els.terminal.classList.add("bell");
        this.events.emit("bell", undefined);
        break;
      case "clipboard":
        void this.backend.writeClipboard(event.text);
        break;
      case "rich_error":
        this.notify(`rich message rejected: ${event.error}`, "error");
        break;
      case "exit":
        this.events.emit("exit", event.code);
        void this.backend.closeWindow();
        break;
    }
  }

  // ---- geometry ------------------------------------------------------------

  private measureGrid() {
    const { clientWidth, clientHeight } = this.els.terminal;
    return this.renderer.fit(clientWidth, clientHeight);
  }

  private scheduleResize() {
    cancelAnimationFrame(this.resizeTimer);
    this.resizeTimer = requestAnimationFrame(() => {
      const { cols, rows } = this.measureGrid();
      if (cols === this.renderer.cols && rows === this.renderer.rows) return;
      this.renderer.resize(cols, rows);
      this.sendResize(cols, rows);
    });
  }

  private sendResize(cols: number, rows: number) {
    if (!this.started) return;
    const { cellWidth, cellHeight } = this.renderer.metrics;
    void this.backend.resize(cols, rows, Math.round(cellWidth), Math.round(cellHeight));
    this.events.emit("resize", { cols, rows });
  }

  private setFontSize(size: number) {
    this.fontSize = Math.min(72, Math.max(6, size));
    this.applyFont();
  }

  private applyFont() {
    const prev = this.renderer.metrics;
    const prevCols = this.renderer.cols;
    const prevRows = this.renderer.rows;
    this.renderer.setFont({ ...this.config.font, size: this.fontSize });
    this.els.cursor.style.fontSize = `${this.fontSize}px`;
    const { cols, rows } = this.measureGrid();
    const next = this.renderer.metrics;
    if (cols !== prevCols || rows !== prevRows || next.cellWidth !== prev.cellWidth || next.cellHeight !== prev.cellHeight) {
      this.renderer.resize(cols, rows);
      this.sendResize(cols, rows);
    } else {
      this.renderer.invalidate();
    }
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
      this.spaceKey = e.key === " ";
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
      const text = normalizeTyped(input.value, this.spaceKey);
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
      if (this.mode & Mode.FOCUS_IN_OUT) void this.backend.write(focused ? "\x1b[I" : "\x1b[O");
      this.events.emit("focus", focused);
    };
    window.addEventListener("focus", () => {
      focusChanged(true);
      if (!document.activeElement || document.activeElement === document.body) input.focus();
    });
    window.addEventListener("blur", () => focusChanged(false));
  }

  private scrollHistory(lines: number) {
    void this.backend.scroll(lines);
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
    if (data) void this.backend.write(data);
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
        void this.backend.readClipboard().then((text) => {
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
          void this.backend.write((prefix + key).repeat(Math.abs(lines)));
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
      version: packageVersion,
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
