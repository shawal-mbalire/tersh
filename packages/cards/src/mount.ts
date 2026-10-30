// Mount a card into any element, outside tersh.

import { builtinRenderers } from "./renderers";
import type { CardInstance, CardMessage, CardRenderer } from "./types";

export interface MountOptions {
  /** Extra or replacement renderers by kind. */
  renderers?: Record<string, CardRenderer>;
  /** Called when the content changes size (an auto-height iframe grew, an image loaded). */
  onResize?: () => void;
}

export interface MountedCard {
  /** Show new content: new `data` (and fields) for the same kind, or a different kind. */
  update(message: CardMessage): Promise<void>;
  dispose(): void;
}

let mounted = 0;

/**
 * Render `message` into `container`, like tersh renders a card's body:
 *
 * ```ts
 * import { mountCard } from "@tersh/cards";
 * import "@tersh/cards/styles.css";
 * await mountCard(el, { kind: "notebook", data: await (await fetch("demo.ipynb")).json() });
 * ```
 */
export async function mountCard(container: HTMLElement, message: CardMessage, options: MountOptions = {}): Promise<MountedCard> {
  const renderers = { ...builtinRenderers, ...options.renderers };
  const id = `card-${++mounted}`;
  let kind = "";
  let instance: CardInstance | void;
  let generation = 0;

  const render = async (msg: CardMessage) => {
    const mine = ++generation;
    instance?.dispose?.();
    container.replaceChildren();
    kind = msg.kind ?? "text";
    const renderer = renderers[kind];
    if (!renderer) throw new Error(`no renderer for card kind "${kind}"`);
    const result = await renderer({ id, kind, data: msg.data, message: msg, el: container, relayout: () => options.onResize?.() });
    if (mine !== generation) result?.dispose?.();
    else instance = result;
  };

  await render(message);
  return {
    async update(msg) {
      if ((msg.kind ?? kind) === kind && instance?.update) await instance.update(msg.data, { ...msg, kind });
      else await render({ ...msg, kind: msg.kind ?? kind });
    },
    dispose() {
      generation++;
      instance?.dispose?.();
      instance = undefined;
      container.replaceChildren();
    },
  };
}
