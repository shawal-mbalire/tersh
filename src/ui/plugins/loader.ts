// Loads user plugins (found by the backend, e.g. in ~/.config/tersh/plugins)
// and reloads them in place when their files change.

import type { Backend, PluginSource } from "../../ports/backend";
import type { Disposable, TershAPI } from "./api";

/**
 * A plugin is an ES module exporting `activate(tersh)` (or a default
 * function). It may return a disposer or `Disposable`, called when the plugin
 * is unloaded; everything it registered through the API is undone
 * automatically. Plugins run with the same privileges as the terminal window,
 * so only install ones you trust.
 */
export type PluginModule = {
  activate?: (api: TershAPI) => unknown;
  default?: (api: TershAPI) => unknown;
};

/** API members whose results are `Disposable`s to undo when a plugin unloads. */
const TRACKED = new Set(["registerRenderer", "registerAction", "registerKeybinding", "onRich", "on", "addStyle"]);

/** A view of the API that remembers what a plugin registered, so it can all be undone. */
export function scopedApi(api: TershAPI, disposables: Disposable[]): TershAPI {
  return new Proxy(api, {
    get(target, key, receiver) {
      const value = Reflect.get(target, key, receiver);
      if (typeof key !== "string" || !TRACKED.has(key) || typeof value !== "function") return value;
      return (...args: unknown[]) => {
        const disposable = (value as (...a: unknown[]) => Disposable).apply(target, args);
        disposables.push(disposable);
        return disposable;
      };
    },
  });
}

function toDisposable(value: unknown): Disposable | null {
  if (typeof value === "function") return { dispose: value as () => void };
  if (value && typeof (value as Disposable).dispose === "function") return value as Disposable;
  return null;
}

/** Evaluate a plugin's source as an ES module. */
export async function importModule(plugin: PluginSource): Promise<PluginModule> {
  const url = URL.createObjectURL(
    new Blob([`${plugin.source}\n//# sourceURL=tersh-plugin://${plugin.name}.js`], { type: "text/javascript" }),
  );
  try {
    return (await import(/* @vite-ignore */ url)) as PluginModule;
  } finally {
    URL.revokeObjectURL(url);
  }
}

export class PluginHost {
  /** Disposables of each loaded plugin, in the order they were created. */
  private loaded = new Map<string, Disposable[]>();

  constructor(
    private readonly api: TershAPI,
    private readonly backend: Pick<Backend, "listPlugins">,
    private readonly importer: (plugin: PluginSource) => Promise<PluginModule> = importModule,
  ) {}

  get names(): string[] {
    return [...this.loaded.keys()];
  }

  /** Load every plugin. */
  async load(): Promise<string[]> {
    for (const plugin of await this.backend.listPlugins()) await this.activate(plugin);
    return this.names;
  }

  /** Unload every plugin and load the current versions. */
  async reload(): Promise<string[]> {
    this.unloadAll();
    return this.load();
  }

  unloadAll() {
    for (const name of [...this.loaded.keys()].reverse()) this.unload(name);
  }

  private unload(name: string) {
    const disposables = this.loaded.get(name) ?? [];
    this.loaded.delete(name);
    for (const disposable of disposables.reverse()) {
      try {
        disposable.dispose();
      } catch (e) {
        console.error(`plugin ${name}: dispose failed`, e);
      }
    }
  }

  private async activate(plugin: PluginSource) {
    const disposables: Disposable[] = [];
    const api = scopedApi(this.api, disposables);
    this.loaded.set(plugin.name, disposables);
    try {
      if (plugin.style) api.addStyle(plugin.style);
      const mod = await this.importer(plugin);
      const activate = mod.activate ?? mod.default;
      if (typeof activate !== "function") throw new Error("plugin must export an `activate(tersh)` function");
      const result = toDisposable(await activate(api));
      if (result) disposables.push(result);
    } catch (e) {
      this.unload(plugin.name);
      console.error(`plugin ${plugin.name}`, e);
      this.api.notify(`plugin "${plugin.name}" failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }
}
