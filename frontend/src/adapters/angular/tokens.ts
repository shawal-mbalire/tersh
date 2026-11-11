// The Angular shell's ports, as injection tokens. The composition root
// (src/providers.ts) binds them with `provideTersh`; tests or other hosts bind
// different adapters without touching the shell.

import { InjectionToken, makeEnvironmentProviders, type EnvironmentProviders } from "@angular/core";
import type { Backend } from "@ports/backend";
import type { Elements } from "@adapters/dom/terminal";
import { bootTerminal, type Booted } from "@adapters/dom/boot";

/** The host: session I/O, settings, clipboard, window (the `Backend` port). */
export const BACKEND = new InjectionToken<Backend>("tersh.backend");

/** Starts a terminal on the shell's elements. Defaults to the canvas page. */
export type TerminalBoot = (backend: Backend, els: Elements) => Promise<Booted>;
export const TERMINAL_BOOT = new InjectionToken<TerminalBoot>("tersh.boot", {
  providedIn: "root",
  factory: () => bootTerminal,
});

export interface TershAdapters {
  /** Creates the host backend, e.g. `() => new TauriBackend()`. */
  backend: () => Backend;
  /** Replaces how the terminal starts (a different screen, extra wiring). */
  boot?: TerminalBoot;
}

/** Plug a set of adapters into the shell. */
export function provideTersh(adapters: TershAdapters): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: BACKEND, useFactory: adapters.backend },
    ...(adapters.boot ? [{ provide: TERMINAL_BOOT, useValue: adapters.boot }] : []),
  ]);
}
