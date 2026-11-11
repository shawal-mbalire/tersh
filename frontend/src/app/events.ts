// Delivers the terminal's events (see `TershEvents`) to plugins. A failing
// handler is logged and doesn't stop the others.

import type { Disposable, TershEvents } from "@domain/events";

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
