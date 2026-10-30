// Types shared by renderers, tersh and any page that mounts cards.

/** Fields of a card message that renderers read; extra fields are passed through. */
export interface CardMessage {
  kind?: string;
  title?: string;
  data?: unknown;
  /** MIME type for base64 `image`/`video` data. */
  mime?: string;
  /** Inside tersh: where the card sits. Renderers only check whether its height is fixed. */
  anchor?: { rows?: number; height?: unknown };
  [key: string]: unknown;
}

export interface CardContext {
  id: string;
  kind: string;
  data: unknown;
  message: CardMessage;
  /** Element to render into. */
  el: HTMLElement;
  /** Ask for the card to be re-laid out (e.g. after content changed size). */
  relayout(): void;
}

export interface CardInstance {
  /** Called with new data on `update`. If absent the card is re-rendered from scratch. */
  update?(data: unknown, message: CardMessage): void | Promise<void>;
  dispose?(): void;
}

export type CardRenderer = (ctx: CardContext) => void | CardInstance | Promise<void | CardInstance>;
