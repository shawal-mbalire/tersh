import type { Frame } from "../types";
import type { Metrics } from "../term/renderer";

/** Where a card sits. Grid anchors are resolved by the core (see `rich.rs`). */
export interface Anchor {
  mode?: "cursor" | "cell" | "float";
  screen?: "primary" | "alt";
  /** Absolute line on the primary screen, screen row on the alternate one. */
  line?: number;
  col?: number;
  /** Size in cells; omitted means "to the right edge" / "fit content". */
  rows?: number;
  cols?: number;
  /** Float anchors: numbers are cells, strings are CSS lengths ("10%", "120px"). */
  top?: number | string;
  left?: number | string;
  width?: number | string;
  height?: number | string;
}

export interface RichMessage {
  op?: "show" | "update" | "move" | "close" | "clear";
  id?: string;
  kind?: string;
  title?: string;
  anchor?: Anchor;
  data?: unknown;
  /** Show the header bar (title + close button). Default true. */
  chrome?: boolean;
  /** Keep the card but don't display it (e.g. its buffer line scrolled out of an editor window). */
  hidden?: boolean;
  /** For `clear`: which cards to remove. Default all. */
  screen?: "primary" | "alt";
  [key: string]: unknown;
}

export interface CardContext {
  id: string;
  kind: string;
  data: unknown;
  message: RichMessage;
  /** Element to render into. */
  el: HTMLElement;
  /** Ask for the card to be re-laid out (e.g. after content changed size). */
  relayout(): void;
}

export interface CardInstance {
  /** Called with new data on `update`. If absent the card is re-rendered from scratch. */
  update?(data: unknown, message: RichMessage): void | Promise<void>;
  dispose?(): void;
}

export type CardRenderer = (ctx: CardContext) => void | CardInstance | Promise<void | CardInstance>;

interface Card {
  id: string;
  message: RichMessage;
  root: HTMLElement;
  body: HTMLElement;
  titleEl: HTMLElement;
  instance: CardInstance | void;
  /** Bumped on every render so stale async renders can be discarded. */
  generation: number;
}

export interface Geometry {
  metrics: Metrics;
  frame: Frame;
}

/** Pixel box of a card relative to the grid's top-left, or null when off-screen. */
export function cardBox(anchor: Anchor, geo: Geometry): { left: number; top: number; width: number | null; height: number | null } | null {
  const { frame, metrics } = geo;
  const { cellWidth: cw, cellHeight: ch } = metrics;
  const gridWidth = frame.cols * cw;
  const gridHeight = frame.rows * ch;

  if (anchor.mode === "float") {
    const len = (v: number | string | undefined, cell: number, fallback: string) =>
      v === undefined ? fallback : typeof v === "number" ? `${v * cell}px` : v;
    const px = (v: string, total: number) => (v.endsWith("%") ? (parseFloat(v) / 100) * total : parseFloat(v) || 0);
    return {
      left: px(len(anchor.left, cw, "10%"), gridWidth),
      top: px(len(anchor.top, ch, "10%"), gridHeight),
      width: px(len(anchor.width, cw, "80%"), gridWidth),
      height: anchor.height === undefined ? null : px(len(anchor.height, ch, "0"), gridHeight),
    };
  }

  const screen = anchor.screen ?? "primary";
  if ((screen === "alt") !== frame.alt) return null;
  const line = anchor.line ?? 0;
  const row = screen === "primary" ? line - (frame.scroll_base - frame.display_offset) : line;
  const col = Math.min(anchor.col ?? 0, frame.cols - 1);
  const rows = anchor.rows;
  if (rows !== undefined && row + rows <= 0) return null;
  if (row >= frame.rows) return null;
  const cols = anchor.cols ?? frame.cols - col;
  return {
    left: col * cw,
    top: row * ch,
    width: Math.min(cols, frame.cols - col) * cw,
    height: rows === undefined ? null : rows * ch,
  };
}

/** Whether a primary-screen card has scrolled out of the history entirely. */
export function isExpired(anchor: Anchor, frame: Frame): boolean {
  if (anchor.mode === "float" || (anchor.screen ?? "primary") !== "primary") return false;
  const rows = anchor.rows ?? 1;
  return (anchor.line ?? 0) + rows <= frame.scroll_base - frame.history;
}

let nextId = 0;

/** Owns the DOM overlay of rich cards and keeps them glued to the grid. */
export class CardManager {
  readonly renderers = new Map<string, CardRenderer>();
  private cards = new Map<string, Card>();
  private geometry: Geometry | null = null;
  private wasAlt = false;

  constructor(
    private readonly container: HTMLElement,
    private readonly onError: (message: string) => void,
  ) {}

  get ids(): string[] {
    return [...this.cards.keys()];
  }

  get(id: string): RichMessage | undefined {
    return this.cards.get(id)?.message;
  }

  handle(msg: RichMessage) {
    switch (msg.op ?? "show") {
      case "show":
        return this.show(msg);
      case "update":
        return this.update(msg);
      case "move": {
        const card = msg.id ? this.cards.get(msg.id) : undefined;
        if (!card) return;
        if (msg.anchor) card.message.anchor = msg.anchor;
        if (msg.hidden !== undefined) card.message.hidden = msg.hidden;
        this.position(card);
        return;
      }
      case "close":
        if (msg.id) this.close(msg.id);
        return;
      case "clear":
        for (const card of [...this.cards.values()]) {
          const screen = card.message.anchor?.screen ?? "primary";
          if (!msg.screen || msg.screen === screen) this.close(card.id);
        }
        return;
      default:
        this.onError(`unknown rich op "${String(msg.op)}"`);
    }
  }

  show(msg: RichMessage): string {
    const id = msg.id ?? `card-${++nextId}`;
    this.close(id);
    const message = { ...msg, id, op: "show" as const };

    const root = document.createElement("div");
    root.className = "card";
    root.dataset.kind = msg.kind ?? "";
    if (msg.chrome === false) root.classList.add("bare");

    const header = document.createElement("header");
    const titleEl = document.createElement("span");
    titleEl.className = "card-title";
    const close = document.createElement("button");
    close.className = "card-close";
    close.title = "Close";
    close.textContent = "×";
    close.addEventListener("click", () => this.close(id));
    header.append(titleEl, close);

    const body = document.createElement("div");
    body.className = "card-body";
    root.append(header, body);
    this.container.append(root);

    const card: Card = { id, message, root, body, titleEl, instance: undefined, generation: 0 };
    this.cards.set(id, card);
    this.setTitle(card);
    this.position(card);
    void this.render(card);
    return id;
  }

  update(msg: RichMessage) {
    const card = msg.id ? this.cards.get(msg.id) : undefined;
    if (!card) {
      // Updating an unknown card creates it, so producers needn't track state.
      if (msg.kind) this.show({ ...msg, op: "show" });
      return;
    }
    const { op: _op, ...changes } = msg;
    Object.assign(card.message, changes);
    this.setTitle(card);
    this.position(card);
    if (!("data" in msg)) return;
    if (card.instance && card.instance.update) {
      Promise.resolve(card.instance.update(msg.data, card.message)).catch((e) => this.renderError(card, e));
    } else {
      void this.render(card);
    }
  }

  close(id: string) {
    const card = this.cards.get(id);
    if (!card) return;
    this.cards.delete(id);
    card.generation++;
    try {
      card.instance?.dispose?.();
    } catch (e) {
      console.error(e);
    }
    card.root.remove();
  }

  /** Re-render every card of a kind, e.g. after a plugin registered a renderer for it. */
  rerender(kind: string) {
    for (const card of this.cards.values()) if (card.message.kind === kind) void this.render(card);
  }

  /** Reposition all cards for the latest frame. Cheap; call on every painted frame. */
  layout(geometry: Geometry) {
    this.geometry = geometry;
    const { frame } = geometry;
    if (this.wasAlt && !frame.alt) {
      // Leaving the alternate screen (e.g. Neovim quit) discards its cards.
      for (const card of [...this.cards.values()]) if (card.message.anchor?.screen === "alt") this.close(card.id);
    }
    this.wasAlt = frame.alt;
    for (const card of [...this.cards.values()]) {
      if (card.message.anchor && isExpired(card.message.anchor, frame)) this.close(card.id);
      else this.position(card);
    }
  }

  private setTitle(card: Card) {
    card.titleEl.textContent = card.message.title ?? card.message.kind ?? "";
  }

  private position(card: Card) {
    const geo = this.geometry;
    const box = geo && !card.message.hidden ? cardBox(card.message.anchor ?? {}, geo) : null;
    const style = card.root.style;
    if (!box) {
      style.display = "none";
      return;
    }
    style.display = "";
    style.transform = `translate(${box.left}px, ${box.top}px)`;
    style.width = box.width === null ? "" : `${box.width}px`;
    style.height = box.height === null ? "" : `${box.height}px`;
    card.root.classList.toggle("fixed-height", box.height !== null);
    if (box.height === null && geo) {
      // Auto-height cards may grow to the bottom of the grid.
      style.maxHeight = `${Math.max(geo.frame.rows * geo.metrics.cellHeight - box.top, geo.metrics.cellHeight * 3)}px`;
    } else {
      style.maxHeight = "";
    }
  }

  private async render(card: Card) {
    const generation = ++card.generation;
    try {
      card.instance?.dispose?.();
    } catch (e) {
      console.error(e);
    }
    card.instance = undefined;
    card.body.replaceChildren();
    card.root.classList.remove("error");

    const kind = card.message.kind ?? "text";
    const renderer = this.renderers.get(kind);
    if (!renderer) {
      this.renderError(card, `no renderer for "${kind}" (a plugin can add one with tersh.registerRenderer)`);
      return;
    }
    try {
      const instance = await renderer({
        id: card.id,
        kind,
        data: card.message.data,
        message: card.message,
        el: card.body,
        relayout: () => this.position(card),
      });
      if (generation !== card.generation) {
        instance?.dispose?.();
        return;
      }
      card.instance = instance;
    } catch (e) {
      if (generation === card.generation) this.renderError(card, e);
    }
  }

  private renderError(card: Card, error: unknown) {
    const text = error instanceof Error ? error.message : String(error);
    card.root.classList.add("error");
    const pre = document.createElement("pre");
    pre.className = "card-error";
    pre.textContent = text;
    card.body.replaceChildren(pre);
    this.onError(`${card.message.kind ?? "card"}: ${text}`);
  }
}
