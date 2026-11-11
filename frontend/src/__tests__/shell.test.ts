// The Angular shell hands its elements and the provided backend to whatever
// boot function is plugged in.

// The tests load Angular from node_modules unlinked; let it JIT-compile the partial declarations.
import "@angular/compiler";
import { provideZonelessChangeDetection } from "@angular/core";
import { bootstrapApplication } from "@angular/platform-browser";
import { describe, expect, it, vi } from "bun:test";
import { TerminalComponent } from "@adapters/angular/terminal.component";
import { provideTersh } from "@adapters/angular/tokens";
import type { Backend } from "@ports/backend";

/** Retry `check` until it passes or a second has gone by. */
async function eventually(check: () => void) {
  for (let tries = 0; ; tries++) {
    try {
      return check();
    } catch (e) {
      if (tries >= 50) throw e;
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  }
}

describe("Angular shell", () => {
  it("boots the plugged-in terminal with the provided backend and its elements", async () => {
    document.body.innerHTML = "<tersh-root></tersh-root>";
    const backend = { windowReady: vi.fn(async () => undefined) } as unknown as Backend;
    const boot = vi.fn(async () => ({}) as never);

    const app = await bootstrapApplication(TerminalComponent, {
      providers: [provideZonelessChangeDetection(), provideTersh({ backend: () => backend, boot })],
    });

    expect(boot).toHaveBeenCalledTimes(1);
    const [given, els] = boot.mock.calls[0] as unknown as [Backend, Record<string, HTMLElement>];
    expect(given).toBe(backend);
    expect(els.canvas).toBe(document.getElementById("grid") as HTMLElement);
    expect(els.input).toBe(document.getElementById("input") as HTMLElement);
    expect(document.querySelector("[data-tauri-drag-region]")).not.toBeNull();
    app.destroy();
  });

  it("shows a startup failure and still lets the host show the window", async () => {
    document.body.innerHTML = "<tersh-root></tersh-root>";
    const backend = { windowReady: vi.fn(async () => undefined) } as unknown as Backend;
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const app = await bootstrapApplication(TerminalComponent, {
      providers: [
        provideZonelessChangeDetection(),
        provideTersh({ backend: () => backend, boot: () => Promise.reject(new Error("no host")) }),
      ],
    });
    await eventually(() => expect(document.body.textContent).toContain("tersh failed to start: no host"));
    expect(backend.windowReady).toHaveBeenCalled();
    app.destroy();
  });
});
