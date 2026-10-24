import { describe, expect, it, vi } from "vitest";
import type { PluginSource } from "../ports/backend";
import type { Disposable, TershAPI } from "../ui/plugins/api";
import { PluginHost, type PluginModule } from "../ui/plugins/loader";

/** Just enough API to see what plugins register and whether it gets undone. */
function fakeApi() {
  const live = new Set<string>();
  const register = (what: string): Disposable => {
    live.add(what);
    return { dispose: () => live.delete(what) };
  };
  const api = {
    registerRenderer: (kind: string) => register(`renderer:${kind}`),
    registerAction: (name: string) => register(`action:${name}`),
    addStyle: (css: string) => register(`style:${css}`),
    notify: vi.fn(),
  } as unknown as TershAPI;
  return { api, live };
}

describe("plugin host", () => {
  it("undoes everything a plugin registered when reloading, then loads the new version", async () => {
    const { api, live } = fakeApi();
    let version = 1;
    const deactivated = vi.fn();
    const sources = (): PluginSource[] => [{ name: "demo", source: "", style: `.v${version} {}` }];
    const importer = async (): Promise<PluginModule> => {
      const v = version;
      return {
        activate(tersh: TershAPI) {
          tersh.registerRenderer(`kind${v}`, () => undefined);
          tersh.registerAction(`demo.v${v}`, () => undefined);
          return deactivated;
        },
      };
    };
    const host = new PluginHost(api, { listPlugins: async () => sources() }, importer);

    expect(await host.load()).toEqual(["demo"]);
    expect([...live].sort()).toEqual(["action:demo.v1", "renderer:kind1", "style:.v1 {}"]);

    version = 2;
    await host.reload();
    expect(deactivated).toHaveBeenCalledTimes(1);
    expect([...live].sort()).toEqual(["action:demo.v2", "renderer:kind2", "style:.v2 {}"]);
  });

  it("reports a broken plugin and leaves nothing of it behind", async () => {
    const { api, live } = fakeApi();
    const importer = async (): Promise<PluginModule> => ({
      activate(tersh: TershAPI) {
        tersh.registerAction("half.done", () => undefined);
        throw new Error("boom");
      },
    });
    const host = new PluginHost(api, { listPlugins: async () => [{ name: "half", source: "", style: null }] }, importer);
    expect(await host.load()).toEqual([]);
    expect(live.size).toBe(0);
    expect(api.notify).toHaveBeenCalledWith('plugin "half" failed: boom', "error");
  });
});
