// The terminal use case on a fake backend and a fake screen: no DOM, no Tauri.

import { describe, expect, it, vi } from "vitest";
import { MAX_FONT_SIZE, TerminalApp } from "../app/terminal";
import type { KeyLike } from "../domain/input";
import { lineAt, rowSpans, wordAt } from "../domain/selection";
import { type Config, type Frame, Mode } from "../domain/types";
import { DELTA_LINE, DELTA_PAGE, wheelLines } from "../domain/wheel";
import type { Backend, SessionHandlers } from "../ports/backend";
import type { Screen } from "../ports/screen";

function config(): Config {
  return {
    shell: null,
    args: [],
    env: {},
    scrollback: 1000,
    font: { family: "monospace", size: 14, line_height: 1.2, bold_is_bright: false },
    window: { padding: 8, opacity: 1, blur: false, width: 800, height: 600 },
    cursor: { shape: "block", blink: false },
    theme: {
      name: "t",
      foreground: "#ffffff",
      background: "#000000",
      cursor: "#ffffff",
      selection: "#333333",
      accent: "#ff0000",
      palette: [],
    },
    keybindings: { "ctrl+shift+k": "clear_cards", "alt+b": { send: "\x1bb" } },
    rich: { enabled: true, osc: true, html_network: false },
    plugins: { disabled: [] },
    update: { auto_restart: true },
    warnings: [],
  };
}

function frame(mode = 0): Frame {
  return {
    cols: 80,
    rows: 24,
    full: true,
    lines: [],
    cursor: { x: 0, y: 0, shape: "block", blink: false },
    mode,
    alt: (mode & Mode.ALT_SCREEN) !== 0,
    display_offset: 0,
    history: 0,
    scroll_base: 0,
  };
}

function rig() {
  const written: string[] = [];
  const scrolled: number[] = [];
  let handlers: SessionHandlers | null = null;
  const backend = {
    startSession: vi.fn(async (_c: number, _r: number, _w: number, _h: number, h: SessionHandlers) => {
      handlers = h;
      return { version: "test", cols: 80, rows: 24 };
    }),
    write: vi.fn(async (data: string) => void written.push(data)),
    resize: vi.fn(async () => undefined),
    scroll: vi.fn(async (lines: number) => void scrolled.push(lines)),
    scrollToBottom: vi.fn(async () => undefined),
    frameShown: vi.fn(async () => undefined),
    readClipboard: vi.fn(async () => "clip"),
    writeClipboard: vi.fn(async () => undefined),
    setTitle: vi.fn(async () => undefined),
    windowReady: vi.fn(async () => undefined),
    reloadConfig: vi.fn(async () => undefined),
    openConfig: vi.fn(async () => undefined),
    restart: vi.fn(async () => undefined),
  } as unknown as Backend;

  const notes: [string, string][] = [];
  const screen = {
    cols: 80,
    rows: 24,
    metrics: { cellWidth: 9, cellHeight: 18, baseline: 14 },
    font: 14,
    fit: () => ({ cols: 80, rows: 24 }),
    resize: vi.fn(),
    setFont(font: Config["font"]) {
      this.font = font.size;
    },
    restyle: vi.fn(),
    show: vi.fn(),
    repaint: vi.fn(),
    rowText: () => "",
    selectionText: () => "selected",
    clearSelection: vi.fn(),
    rich: vi.fn(),
    notify: (message: string, level: string) => void notes.push([message, level]),
    bell: vi.fn(),
  };

  const app = new TerminalApp<KeyLike>(config(), backend, screen as Screen, false);
  const session = () => handlers!;
  return { app, backend, screen, written, scrolled, notes, session };
}

const key = (k: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key: k,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});
const noMods = { shift: false, alt: false, ctrl: false };

describe("TerminalApp", () => {
  it("gives a key to plugins first (newest first), then bindings, then the program", async () => {
    const { app, written, screen } = rig();
    const order: string[] = [];
    app.registerKeybinding("ctrl+shift+k", () => void order.push("old"));
    app.registerKeybinding("ctrl+shift+k", () => (order.push("new"), false));
    expect(app.key(key("K", { ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(order).toEqual(["new", "old"]);
    expect(screen.rich).not.toHaveBeenCalled();

    // No plugin handler: the config binds it to an action.
    const other = rig();
    other.app.key(key("K", { ctrlKey: true, shiftKey: true }));
    expect(other.screen.rich).toHaveBeenCalledWith({ op: "clear" });

    // `{ send = ... }` bindings type text; special keys are encoded for the
    // program; printable ones arrive as typed text instead.
    app.key(key("b", { altKey: true }));
    expect(app.key(key("Enter"))).toBe(true);
    expect(app.key(key("a"))).toBe(false);
    app.typed("a");
    expect(app.keyUp(key("a"))).toBe(false);
    expect(written).toEqual(["\x1bb", "\r", "a"]);
  });

  it("reports failing and unknown actions", async () => {
    const { app, notes } = rig();
    app.registerAction("boom", () => {
      throw new Error("nope");
    });
    expect(app.runAction("boom")).toBe(true);
    expect(app.runAction("missing")).toBe(false);
    expect(notes.map(([m]) => m)).toEqual(["boom: nope", 'unknown action "missing" (see config.toml for the list)']);
  });

  it("scrolls history, sends arrow keys or reports the wheel, depending on the program", async () => {
    const { app, session, written, scrolled } = rig();
    await app.start();
    app.wheel(3, { x: 0, y: 0 }, noMods);
    expect(scrolled).toEqual([-3]);

    session().frame(frame(Mode.ALT_SCREEN | Mode.ALTERNATE_SCROLL));
    app.wheel(-2, { x: 0, y: 0 }, noMods);
    session().frame(frame(Mode.MOUSE_REPORT_CLICK | Mode.SGR_MOUSE));
    app.wheel(1, { x: 4, y: 2 }, noMods);
    expect(written).toEqual(["\x1b[A\x1b[A", "\x1b[<65;5;3M"]);
  });

  it("reports focus and mouse buttons when the program asks", async () => {
    const { app, session, written } = rig();
    await app.start();
    app.focusChanged(false);
    expect(written).toEqual([]);
    session().frame(frame(Mode.FOCUS_IN_OUT | Mode.MOUSE_REPORT_CLICK | Mode.SGR_MOUSE));
    app.focusChanged(true);
    app.mouse("press", { x: 0, y: 0 }, 0, noMods);
    expect(app.mouseButton).toBe(0);
    app.mouse("release", { x: 0, y: 0 }, 0, noMods);
    expect(app.mouseButton).toBe(null);
    expect(written).toEqual(["\x1b[I", "\x1b[<0;1;1M", "\x1b[<0;1;1m"]);
  });

  it("keeps zoom within bounds and tells the host the window is ready once", async () => {
    const { app, screen, backend } = rig();
    for (let i = 0; i < 100; i++) app.runAction("zoom_in");
    expect(screen.font).toBe(MAX_FONT_SIZE);

    app.painted();
    expect(backend.windowReady).not.toHaveBeenCalled();
    await app.start();
    app.painted();
    app.painted();
    expect(backend.windowReady).not.toHaveBeenCalled(); // no frame yet
    // Frames arrive through the session.
    const { session } = { session: () => (backend.startSession as ReturnType<typeof vi.fn>).mock.calls[0][4] as SessionHandlers };
    session().frame(frame());
    app.painted();
    app.painted();
    expect(backend.windowReady).toHaveBeenCalledTimes(1);
  });

  it("acknowledges each painted frame once, so the session sends the next", async () => {
    const { app, backend, session } = rig();
    await app.start();
    app.painted();
    expect(backend.frameShown).not.toHaveBeenCalled(); // nothing received yet
    session().frame(frame());
    session().frame(frame()); // two frames, one paint: one acknowledgement
    app.painted();
    app.painted();
    expect(backend.frameShown).toHaveBeenCalledTimes(1);
    session().frame(frame());
    app.painted();
    expect(backend.frameShown).toHaveBeenCalledTimes(2);
  });
});

describe("selection", () => {
  it("spans rows and finds words", () => {
    const s = { anchor: { x: 5, y: 2 }, head: { x: 3, y: 0 } };
    expect(rowSpans(s, 10)).toEqual([
      { y: 0, from: 3, to: 10 },
      { y: 1, from: 0, to: 10 },
      { y: 2, from: 0, to: 6 },
    ]);
    expect(wordAt("cd ~/work/tersh; ls", 6, 1)).toEqual({ anchor: { x: 3, y: 1 }, head: { x: 14, y: 1 } });
    expect(wordAt("a b", 1, 0)).toBeNull();
    expect(lineAt(4, 80)).toEqual({ anchor: { x: 0, y: 4 }, head: { x: 79, y: 4 } });
  });
});

describe("wheel", () => {
  it("counts whole lines and carries the rest", () => {
    expect(wheelLines({ deltaY: 3, deltaMode: DELTA_LINE }, 0, 24, 18)).toEqual({ lines: 3, remainder: 0 });
    expect(wheelLines({ deltaY: 1, deltaMode: DELTA_PAGE }, 0, 24, 18)).toEqual({ lines: 24, remainder: 0 });
    const first = wheelLines({ deltaY: 27, deltaMode: 0 }, 0, 24, 18);
    expect(first).toEqual({ lines: 1, remainder: 0.5 });
    expect(wheelLines({ deltaY: 9, deltaMode: 0 }, first.remainder, 24, 18).lines).toBe(1);
  });
});
