import { describe, expect, it } from "vitest";
import { cardBox, CardManager, isExpired, type CardRenderer } from "../cards/manager";
import type { Frame } from "../types";

const frame = (over: Partial<Frame> = {}): Frame => ({
  cols: 80,
  rows: 24,
  full: false,
  lines: [],
  cursor: { x: 0, y: 0, shape: "block", blink: false },
  mode: 0,
  alt: false,
  display_offset: 0,
  history: 100,
  scroll_base: 100,
  ...over,
});
const metrics = { cellWidth: 10, cellHeight: 20, baseline: 15 };

describe("cardBox", () => {
  it("places primary-screen cards relative to scrolled output", () => {
    // Created at screen row 5 when 100 lines had scrolled away.
    const anchor = { screen: "primary" as const, line: 105, col: 2, rows: 4 };
    expect(cardBox(anchor, { frame: frame(), metrics })).toEqual({ left: 20, top: 100, width: 780, height: 80 });
    // Three more lines of output push it up.
    expect(cardBox(anchor, { frame: frame({ scroll_base: 103 }), metrics })?.top).toBe(40);
    // Scrolling back through history brings it down again.
    expect(cardBox(anchor, { frame: frame({ scroll_base: 103, display_offset: 3 }), metrics })?.top).toBe(100);
  });

  it("hides cards scrolled off either edge", () => {
    const anchor = { screen: "primary" as const, line: 105, rows: 4 };
    expect(cardBox(anchor, { frame: frame({ scroll_base: 110 }), metrics })).toBeNull();
    expect(cardBox(anchor, { frame: frame({ scroll_base: 80 }), metrics })).toBeNull();
    // Partially visible at the top is still shown.
    expect(cardBox(anchor, { frame: frame({ scroll_base: 107 }), metrics })?.top).toBe(-40);
  });

  it("only shows cards for the active screen", () => {
    expect(cardBox({ screen: "alt", line: 3 }, { frame: frame(), metrics })).toBeNull();
    expect(cardBox({ screen: "alt", line: 3, col: 10, cols: 20 }, { frame: frame({ alt: true }), metrics })).toEqual({
      left: 100,
      top: 60,
      width: 200,
      height: null,
    });
    expect(cardBox({ screen: "primary", line: 100 }, { frame: frame({ alt: true }), metrics })).toBeNull();
  });

  it("sizes float cards in cells or CSS units", () => {
    const box = cardBox({ mode: "float", left: 2, top: "50%", width: "200px", height: 5 }, { frame: frame(), metrics });
    expect(box).toEqual({ left: 20, top: 240, width: 200, height: 100 });
  });
});

describe("isExpired", () => {
  it("expires cards that left the scrollback", () => {
    expect(isExpired({ line: 10, rows: 2 }, frame({ scroll_base: 1000, history: 500 }))).toBe(true);
    expect(isExpired({ line: 600, rows: 2 }, frame({ scroll_base: 1000, history: 500 }))).toBe(false);
    expect(isExpired({ screen: "alt", line: 0 }, frame({ scroll_base: 1000, history: 0 }))).toBe(false);
  });
});

describe("CardManager", () => {
  it("creates, updates and closes cards", async () => {
    const container = document.createElement("div");
    const errors: string[] = [];
    const manager = new CardManager(container, (e) => errors.push(e));
    const updates: unknown[] = [];
    let disposed = 0;
    const renderer: CardRenderer = ({ el, data }) => {
      el.textContent = String(data);
      return { update: (d) => void updates.push(d), dispose: () => void disposed++ };
    };
    manager.renderers.set("test", renderer);

    manager.handle({ op: "show", id: "a", kind: "test", data: "hello", title: "Greeting" });
    await Promise.resolve();
    expect(container.querySelector(".card-body")?.textContent).toBe("hello");
    expect(container.querySelector(".card-title")?.textContent).toBe("Greeting");

    manager.handle({ op: "update", id: "a", data: "again" });
    expect(updates).toEqual(["again"]);

    manager.handle({ op: "close", id: "a" });
    expect(container.children).toHaveLength(0);
    expect(disposed).toBe(1);
    expect(errors).toEqual([]);
  });

  it("hides and re-shows cards on move", () => {
    const container = document.createElement("div");
    const manager = new CardManager(container, () => {});
    manager.renderers.set("x", () => {});
    manager.layout({ frame: frame({ alt: true }), metrics });
    manager.handle({ id: "c", kind: "x", anchor: { screen: "alt", line: 2 } });
    const root = container.querySelector<HTMLElement>(".card")!;
    expect(root.style.display).toBe("");
    manager.handle({ op: "move", id: "c", hidden: true });
    expect(root.style.display).toBe("none");
    manager.handle({ op: "move", id: "c", hidden: false, anchor: { screen: "alt", line: 5 } });
    expect(root.style.display).toBe("");
    expect(root.style.transform).toBe("translate(0px, 100px)");
  });

  it("reports unknown kinds inside the card", async () => {
    const container = document.createElement("div");
    const errors: string[] = [];
    const manager = new CardManager(container, (e) => errors.push(e));
    manager.handle({ kind: "mystery" });
    await Promise.resolve();
    expect(container.querySelector(".card-error")?.textContent).toContain('no renderer for "mystery"');
    expect(errors).toHaveLength(1);
  });

  it("drops alternate-screen cards when the program exits", async () => {
    const container = document.createElement("div");
    const manager = new CardManager(container, () => {});
    manager.renderers.set("x", () => {});
    manager.handle({ id: "alt", kind: "x", anchor: { screen: "alt", line: 0 } });
    manager.handle({ id: "pri", kind: "x", anchor: { screen: "primary", line: 100 } });
    manager.layout({ frame: frame({ alt: true }), metrics });
    manager.layout({ frame: frame({ alt: false }), metrics });
    expect(manager.ids).toEqual(["pri"]);
  });
});
