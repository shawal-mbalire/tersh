import type { CardInstance, CardRenderer } from "@tersh/cards";
import { type Anchor, cardBox, type Geometry, isExpired, type RichMessage } from "../../domain/cards";

export type { Anchor, RichMessage };
export type { CardContext, CardInstance, CardRenderer } from "@tersh/cards";

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
