// Loads user plugins from ~/.config/tersh/plugins (discovered by the Rust side).

import { invoke } from "@tauri-apps/api/core";
import type { TershAPI } from "./api";

interface PluginSource {
  name: string;
  source: string;
  style: string | null;
}

/**
 * A plugin is an ES module exporting `activate(tersh)` (or a default
 * function). It may return a disposer or `Disposable`. Plugins run with the
 * same privileges as the terminal window — only install ones you trust.
 */
type PluginModule = {
  activate?: (api: TershAPI) => unknown;
  default?: (api: TershAPI) => unknown;
};

export async function loadPlugins(api: TershAPI): Promise<string[]> {
  const plugins = await invoke<PluginSource[]>("list_plugins");
  const loaded: string[] = [];
  for (const plugin of plugins) {
    try {
      if (plugin.style) api.addStyle(plugin.style);
      const url = URL.createObjectURL(
        new Blob([`${plugin.source}\n//# sourceURL=tersh-plugin://${plugin.name}.js`], { type: "text/javascript" }),
      );
      try {
        const mod = (await import(/* @vite-ignore */ url)) as PluginModule;
        const activate = mod.activate ?? mod.default;
        if (typeof activate !== "function") throw new Error("plugin must export an `activate(tersh)` function");
        await activate(api);
      } finally {
        URL.revokeObjectURL(url);
      }
      loaded.push(plugin.name);
    } catch (e) {
      console.error(`plugin ${plugin.name}`, e);
      api.notify(`plugin "${plugin.name}" failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    }
  }
  return loaded;
}
