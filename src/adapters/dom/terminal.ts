// The page: draws the terminal (canvas grid, cursor, selection, cards, toasts)
// as the use case's Screen, and turns keyboard, mouse, focus and resize events
// into calls on the use case.

import { builtinRenderers } from "@tersh/cards";
import type { TerminalApp } from "../../app/terminal";
import type { RichMessage } from "../../domain/cards";
import { effectiveKey } from "../../domain/input";
import { Palette } from "../../domain/palette";
import { type Cell, isEmpty, lineAt, rowSpans, type Selection, wordAt } from "../../domain/selection";
import type { Config, Frame, Metrics } from "../../domain/types";
import { wheelLines } from "../../domain/wheel";
import type { Backend } from "../../ports/backend";
import type { NotifyLevel, Screen } from "../../ports/screen";
import { CardManager } from "./cards/manager";
import { Profiler } from "./profiler";
import { GridRenderer } from "./renderer";

export const isMac = navigator.userAgent.includes("Mac");

/** How long toasts stay up. */
const TOAST_MS = { info: 4000, error: 8000 };

export interface Elements {
  terminal: HTMLElement;
  canvas: HTMLCanvasElement;
  cursor: HTMLElement;
  selection: HTMLElement;
  cards: HTMLElement;
  input: HTMLTextAreaElement;
  toasts: HTMLElement;
}

export class DomScreen implements Screen {
  readonly renderer: GridRenderer;
  readonly cards: CardManager;
  readonly profiler: Profiler;

  private app: TerminalApp<KeyboardEvent> | null = null;
  private paintPending = false;
  private resizeTimer = 0;
  private composing = false;
  private wheelRemainder = 0;
  private selection: Selection | null = null;
  private selecting = false;

  constructor(
    config: Config,
    backend: Backend,
    private readonly els: Elements,
  ) {
    this.renderer = new GridRenderer(
      els.canvas,
      els.cursor,
      config.font,
      new Palette(config.theme, config.font.bold_is_bright),
      config.window.opacity,
    );
    this.cards = new CardManager(els.cards, (message) => this.notify(message, "error"));
    for (const [kind, renderer] of Object.entries(builtinRenderers)) this.cards.renderers.set(kind, renderer);
    this.profiler = new Profiler(
      document.body,
      () => backend.stats(),
      () => this.cards.ids.length,
    );
  }

  /** Start sending input to `app`, and register the actions only the page can do. */
  attach(app: TerminalApp<KeyboardEvent>) {
    this.app = app;
    app.registerAction("toggle_profiler", () => this.profiler.toggle());
    this.bindInput(app);
    this.bindMouse(app);
  }

  /** After the session started: follow the window's size, and take the keyboard. */
  started() {
    new ResizeObserver(() => this.scheduleResize()).observe(this.els.terminal);
    this.els.input.focus();
  }

  // ---- Screen ----------------------------------------------------------------

  get cols(): number {
    return this.renderer.cols;
  }

  get rows(): number {
    return this.renderer.rows;
  }

  get metrics(): Metrics {
    return this.renderer.metrics;
  }

  get frame(): Frame | null {
    return this.renderer.frame;
  }

  fit() {
    let { clientWidth: width, clientHeight: height } = this.els.terminal;
    if (width === 0 || height === 0) {
      // Not laid out yet (e.g. the window is still hidden): use the window's size less the padding.
      const { offsetLeft, offsetTop } = this.els.terminal;
      width = window.innerWidth - 2 * offsetLeft;
      height = window.innerHeight - 2 * offsetTop;
    }
    return this.renderer.fit(width, height);
  }

  resize(cols: number, rows: number) {
    this.renderer.resize(cols, rows);
  }

  setFont(font: Config["font"]) {
    this.renderer.setFont(font);
    this.els.cursor.style.fontSize = `${font.size}px`;
  }

  restyle(palette: Palette, opacity: number) {
    this.renderer.palette = palette;
    this.renderer.opacity = opacity;
  }

  show(frame: Frame) {
    const scrolled = this.renderer.frame && this.renderer.frame.display_offset !== frame.display_offset;
    this.renderer.apply(frame);
    if (scrolled) this.clearSelection();
    this.schedulePaint();
  }

  repaint() {
    this.renderer.invalidate();
    this.schedulePaint();
  }

  rowText(y: number, from?: number, to?: number): string {
    return this.renderer.rowText(y, from, to);
  }

  selectionText(): string {
    if (!this.selection) return "";
    return rowSpans(this.selection, this.cols)
      .map(({ y, from, to }) => this.renderer.rowText(y, from, to).trimEnd())
      .join("\n");
  }

  clearSelection() {
    if (!this.selection) return;
    this.selection = null;
    this.selecting = false;
    this.drawSelection();
  }

  rich(message: RichMessage) {
    this.cards.handle(message);
  }

  notify(message: string, level: NotifyLevel = "info") {
    const toast = document.createElement("div");
    toast.className = `toast ${level}`;
    toast.textContent = message;
    this.els.toasts.append(toast);
    setTimeout(() => toast.remove(), TOAST_MS[level]);
  }

  bell() {
    const el = this.els.terminal;
    el.classList.remove("bell");
    void el.offsetWidth; // restart the animation
    el.classList.add("bell");
  }

  // ---- painting --------------------------------------------------------------

  private schedulePaint() {
    if (this.paintPending) return;
    this.paintPending = true;
    requestAnimationFrame(() => {
      this.paintPending = false;
      const start = performance.now();
      this.renderer.draw();
      const frame = this.renderer.frame;
      if (frame) {
        this.cards.layout({ frame, metrics: this.renderer.metrics });
        this.placeInput(frame);
      }
      this.drawSelection();
      this.profiler.painted(start, performance.now());
      this.app?.painted();
    });
  }

  private scheduleResize() {
    cancelAnimationFrame(this.resizeTimer);
    this.resizeTimer = requestAnimationFrame(() => this.app?.resized());
  }

  /** Keep the hidden textarea at the cursor so IME candidate windows appear there. */
  private placeInput(frame: Frame) {
    const { cellWidth, cellHeight } = this.renderer.metrics;
    this.els.input.style.transform = `translate(${frame.cursor.x * cellWidth}px, ${frame.cursor.y * cellHeight}px)`;
    this.els.input.style.height = `${cellHeight}px`;
  }

  private drawSelection() {
    const layer = this.els.selection;
    if (!this.selection) {
      layer.replaceChildren();
      return;
    }
    const { cellWidth: cw, cellHeight: ch } = this.renderer.metrics;
    const boxes = rowSpans(this.selection, this.cols).map(({ y, from, to }) => {
      const box = document.createElement("div");
      box.style.transform = `translate(${from * cw}px, ${y * ch}px)`;
      box.style.width = `${(to - from) * cw}px`;
      box.style.height = `${ch}px`;
      return box;
    });
    layer.replaceChildren(...boxes);
  }

  private select(selection: Selection | null) {
    this.selection = selection;
    this.drawSelection();
  }

  private cellFromEvent(e: MouseEvent): Cell {
    const rect = this.els.canvas.getBoundingClientRect();
    const { cellWidth, cellHeight } = this.renderer.metrics;
    return {
      x: Math.min(this.cols - 1, Math.max(0, Math.floor((e.clientX - rect.left) / cellWidth))),
      y: Math.min(this.rows - 1, Math.max(0, Math.floor((e.clientY - rect.top) / cellHeight))),
    };
  }

  // ---- keyboard --------------------------------------------------------------

  private bindInput(app: TerminalApp<KeyboardEvent>) {
    const input = this.els.input;
    const inCard = (target: EventTarget | null) => Boolean((target as HTMLElement | null)?.closest?.(".card"));

    window.addEventListener("keydown", (e) => {
      if (inCard(e.target)) return;
      if (document.activeElement !== input) input.focus();
      if (e.isComposing || this.composing || e.key === "Dead" || e.keyCode === 229) return;
      if (app.key(e)) {
        e.preventDefault();
        return;
      }
      // Bypass WebKit's <textarea> editing engine for direct printable keystrokes
      // so WebKit never rewrites spaces to U+00A0 or drops keys when unfocused.
      const key = effectiveKey(e);
      if (!e.ctrlKey && !e.altKey && !e.metaKey && [...key].length === 1) {
        e.preventDefault();
        input.value = "";
        app.typed(key);
      }
    });
    window.addEventListener("keyup", (e) => {
      if (inCard(e.target)) return;
      if (e.isComposing || this.composing || e.key === "Dead" || e.keyCode === 229) return;
      if (app.keyUp(e)) e.preventDefault();
    });
    input.addEventListener("input", () => {
      if (this.composing) return;
      const text = input.value;
      input.value = "";
      app.typed(text);
    });
    input.addEventListener("compositionstart", () => (this.composing = true));
    input.addEventListener("compositionend", (e) => {
      this.composing = false;
      input.value = "";
      app.composed(e.data);
    });
    window.addEventListener("paste", (e) => {
      if (inCard(e.target)) return;
      e.preventDefault();
      const text = e.clipboardData?.getData("text/plain");
      if (text) void app.paste(text);
    });

    const focusChanged = (focused: boolean) => {
      this.els.terminal.classList.toggle("unfocused", !focused);
      app.focusChanged(focused);
    };
    window.addEventListener("focus", () => {
      focusChanged(true);
      if (!inCard(document.activeElement)) input.focus();
    });
    window.addEventListener("blur", () => focusChanged(false));
  }

  // ---- mouse -----------------------------------------------------------------

  private bindMouse(app: TerminalApp<KeyboardEvent>) {
    const surface = this.els.terminal;
    const onGrid = (e: Event) => !(e.target as HTMLElement).closest(".card");
    const mods = (e: MouseEvent) => ({ shift: e.shiftKey, alt: e.altKey, ctrl: e.ctrlKey });

    surface.addEventListener("mousedown", (e) => {
      if (!onGrid(e)) return;
      e.preventDefault();
      this.els.input.focus();
      const cell = this.cellFromEvent(e);
      if (app.reportsMouse(e.shiftKey)) {
        app.mouse("press", cell, e.button, mods(e));
        return;
      }
      if (e.button === 1) {
        app.pasteClipboard();
        return;
      }
      if (e.button !== 0) return;
      if (e.detail === 2) {
        const word = wordAt(this.renderer.rowText(cell.y), cell.x, cell.y);
        if (word) this.select(word);
        return;
      }
      if (e.detail >= 3) {
        this.select(lineAt(cell.y, this.cols));
        return;
      }
      this.select({ anchor: cell, head: cell });
      this.selecting = true;
    });

    window.addEventListener("mousemove", (e) => {
      if (this.selecting && this.selection) {
        this.select({ ...this.selection, head: this.cellFromEvent(e) });
      } else if (app.reportsMouse(e.shiftKey) && onGrid(e)) {
        app.mouse("move", this.cellFromEvent(e), app.mouseButton ?? 0, mods(e));
      }
    });

    window.addEventListener("mouseup", (e) => {
      const button = app.mouseButton;
      if (button !== null) app.mouse("release", this.cellFromEvent(e), button, mods(e));
      if (this.selecting) {
        this.selecting = false;
        if (this.selection && isEmpty(this.selection)) this.clearSelection();
      }
    });

    surface.addEventListener(
      "wheel",
      (e) => {
        if (!onGrid(e)) return;
        e.preventDefault();
        const { lines, remainder } = wheelLines(e, this.wheelRemainder, this.rows, this.metrics.cellHeight);
        this.wheelRemainder = remainder;
        if (lines !== 0) app.wheel(lines, this.cellFromEvent(e), mods(e));
      },
      { passive: false },
    );
  }
}
