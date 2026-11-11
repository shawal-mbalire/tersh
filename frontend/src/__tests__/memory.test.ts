// Memory leak checks for the page: things that come and go (cards, HTML
// documents, listeners, profiler samples) must not pile up.

import { describe, expect, it } from "bun:test";
import { builtinRenderers, setDocumentHost } from "@tersh/cards";
import { CardManager, type CardRenderer } from "@adapters/dom/cards/manager";
import { Emitter } from "@adapters/dom/plugins/api";
import { Profiler } from "@adapters/dom/profiler";
import { Samples } from "@domain/profile";
import type { Frame, SessionStats } from "@domain/types";

const frame = (over: Partial<Frame> = {}): Frame => ({
  cols: 80,
  rows: 24,
  full: false,
  lines: [],
  cursor: { x: 0, y: 0, shape: "block", blink: false },
  mode: 0,
  alt: false,
  display_offset: 0,
  history: 0,
  scroll_base: 0,
  ...over,
});
const metrics = { cellWidth: 10, cellHeight: 20, baseline: 15 };
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("cards", () => {
  it("leave nothing behind once closed, cleared or replaced", async () => {
    const container = document.createElement("div");
    const manager = new CardManager(container, () => undefined);
    let live = 0;
    const counted: CardRenderer = (ctx) => {
      live++;
      ctx.el.textContent = String(ctx.data);
      return { dispose: () => void live-- };
    };
    manager.renderers.set("counted", counted);

    for (let i = 0; i < 500; i++) {
      manager.show({ kind: "counted", data: i, anchor: { mode: "float" } });
      manager.update({ id: "same", kind: "counted", data: i });
    }
    await settle();
    manager.layout({ frame: frame(), metrics });
    expect(manager.ids.length).toBe(501);
    expect(live).toBe(501);

    manager.handle({ op: "clear" });
    expect(manager.ids).toEqual([]);
    expect(container.childElementCount).toBe(0);
    expect(live).toBe(0);
  });

  it("dispose a render that finishes after the card closed", async () => {
    const manager = new CardManager(document.createElement("div"), () => undefined);
    let live = 0;
    manager.renderers.set("slow", async () => {
      await settle();
      live++;
      return { dispose: () => void live-- };
    });
    for (let i = 0; i < 50; i++) manager.close(manager.show({ kind: "slow", data: i }));
    await settle();
    await settle();
    expect(live).toBe(0);
  });

  it("drop the HTML documents they stored", async () => {
    const stored = new Set<string>();
    setDocumentHost({
      put: async (id) => void stored.add(id),
      drop: (id) => void stored.delete(id),
      url: () => "about:blank",
    });
    const manager = new CardManager(document.createElement("div"), () => undefined);
    manager.renderers.set("html", builtinRenderers.html);
    const ids: string[] = [];
    for (let i = 0; i < 30; i++) ids.push(manager.show({ kind: "html", data: `<p>${i}</p>` }));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(stored.size).toBe(30);
    for (const id of ids) manager.close(id);
    expect(stored.size).toBe(0);
  });
});

describe("listeners", () => {
  it("are removed when disposed", () => {
    const emitter = new Emitter();
    const handlers = (emitter as unknown as { handlers: Map<string, Set<unknown>> }).handlers;
    for (let i = 0; i < 1000; i++) emitter.on("frame", () => undefined).dispose();
    expect(handlers.get("frame")?.size).toBe(0);
  });
});

describe("profiler", () => {
  it("keeps a bounded number of samples however long it runs", () => {
    const samples = new Samples(100);
    for (let i = 0; i < 10_000; i++) samples.add(i);
    expect(samples.count).toBe(100);

    const stats = async () => ({}) as SessionStats;
    const profiler = new Profiler(document.body, stats, () => 0);
    for (let i = 0; i < 20_000; i++) {
      profiler.frame();
      profiler.input();
      profiler.painted(0, performance.now());
    }
    const inner = profiler as unknown as { paints: Samples; latencies: Samples };
    expect(inner.paints.count).toBeLessThanOrEqual(240);
    expect(inner.latencies.count).toBeLessThanOrEqual(240);
  });

  it("forgets timestamps older than a second", async () => {
    const profiler = new Profiler(
      document.body,
      async () => ({}) as SessionStats,
      () => 0,
    );
    const inner = profiler as unknown as { frameTimes: number[] };
    const start = performance.now();
    inner.frameTimes.push(start - 5000, start - 3000);
    profiler.frame();
    expect(inner.frameTimes.length).toBe(1);
  });
});
