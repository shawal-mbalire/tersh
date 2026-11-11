// Starts the terminal on a set of page elements and a host backend. Framework
// free: the Angular shell (adapters/angular) calls it with its view's elements
// and whichever Backend the composition root provided.

import { setDocumentHost } from "@tersh/cards";
import { TerminalApp } from "@app/terminal";
import type { Settings } from "@domain/types";
import type { Backend } from "@ports/backend";
import { loadFonts } from "./fonts";
import { createApi } from "./plugins/api";
import { PluginHost } from "./plugins/loader";
import { profiled } from "./profiler";
import { applySettings } from "./settings";
import { DomScreen, type Elements, isMac } from "./terminal";

/** Show config problems without burying the terminal in toasts. */
function reportWarnings(terminal: TerminalApp<KeyboardEvent>, settings: Settings) {
  const { warnings } = settings.config;
  for (const warning of warnings.slice(0, 3)) terminal.notify(warning, "error");
  if (warnings.length > 3) terminal.notify(`…and ${warnings.length - 3} more problems in ${settings.config_path}`, "error");
}

export interface Booted {
  terminal: TerminalApp<KeyboardEvent>;
  screen: DomScreen;
  plugins: PluginHost;
}

export async function bootTerminal(backend: Backend, els: Elements): Promise<Booted> {
  setDocumentHost(backend.cardDocuments);
  const settings = await backend.getSettings();
  applySettings(settings);

  const screen = new DomScreen(settings.config, backend, els);
  const terminal = new TerminalApp<KeyboardEvent>(settings.config, profiled(backend, screen.profiler), screen, isMac);
  screen.attach(terminal);
  const api = createApi(terminal, screen, backend);

  // Start the shell and load fonts concurrently so the shell initializes while
  // WebKit finishes rasterising the bundled font faces.
  const fontsReady = loadFonts(settings.config.font).then(() => document.fonts.ready);
  await Promise.all([terminal.start().then(() => screen.started()), fontsReady]);
  terminal.refreshFonts();
  (window as unknown as { tersh: unknown }).tersh = api;
  reportWarnings(terminal, settings);

  // Some compositors (Wayland in particular) drop what a transparent window
  // drew while it was unmapped; repaint whenever it comes back.
  await backend.onWindowShown?.(() => terminal.redraw());
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) terminal.redraw();
  });

  const plugins = new PluginHost(api, backend);
  let current = settings;

  // Anything in the settings directory, plugins included, or the installed fonts changed.
  await backend.onSettingsChanged(async (next) => {
    const previous = current;
    current = next;
    applySettings(next);
    await loadFonts(next.config.font);
    terminal.reconfigure(next.config);
    if (next.fonts_revision !== previous.fonts_revision) terminal.refreshFonts();

    const changes: string[] = [];
    if (next.plugins_revision !== previous.plugins_revision) {
      const loaded = await plugins.reload();
      changes.push(`plugins reloaded (${loaded.length})`);
    }
    if (next.fonts_revision !== previous.fonts_revision) changes.push("fonts updated");
    if (next.config.warnings.length) reportWarnings(terminal, next);
    else terminal.notify(changes.length ? changes.join(" · ") : `config reloaded · theme ${next.config.theme.name}`);
  });

  await backend.onAppUpdated((version) => {
    const restart = current.config.update.auto_restart
      ? "restarting as soon as the shell is idle, or run the restart action (Ctrl+Shift+F5) now"
      : "run the restart action (Ctrl+Shift+F5) to switch";
    terminal.notify(`tersh ${version} installed: ${restart}`);
  });

  await plugins.load();
  return { terminal, screen, plugins };
}

/** Replace the page with the startup error, and still let the host show the window. */
export function showFatal(backend: Backend, error: unknown) {
  console.error(error);
  document.body.textContent = `tersh failed to start: ${error instanceof Error ? error.message : String(error)}`;
  document.body.classList.add("fatal");
  void backend.windowReady().catch(() => undefined);
}
