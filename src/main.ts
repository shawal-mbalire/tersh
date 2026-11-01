// Composition root: the Tauri backend and the page, wired to the terminal use case.

import "@fontsource-variable/sono/wght.css";
import { TauriBackend } from "./adapters/tauri/backend";
import { setDocumentHost } from "@tersh/cards";
import { loadFonts } from "./adapters/dom/fonts";
import { PluginHost } from "./adapters/dom/plugins/loader";
import { applySettings } from "./adapters/dom/settings";
import { DomScreen, isMac } from "./adapters/dom/terminal";
import { createApi } from "./adapters/dom/plugins/api";
import { profiled } from "./adapters/dom/profiler";
import { TerminalApp } from "./app/terminal";
import type { Settings } from "./domain/types";

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/** Show config problems without burying the terminal in toasts. */
function reportWarnings(terminal: TerminalApp<KeyboardEvent>, settings: Settings) {
  const { warnings } = settings.config;
  for (const warning of warnings.slice(0, 3)) terminal.notify(warning, "error");
  if (warnings.length > 3) terminal.notify(`…and ${warnings.length - 3} more problems in ${settings.config_path}`, "error");
}

const backend = new TauriBackend();

async function main() {
  setDocumentHost(backend.cardDocuments);
  const settings = await backend.getSettings();
  applySettings(settings);

  const screen = new DomScreen(settings.config, backend, {
    terminal: byId("terminal"),
    canvas: byId<HTMLCanvasElement>("grid"),
    cursor: byId("cursor"),
    selection: byId("selection"),
    cards: byId("cards"),
    input: byId<HTMLTextAreaElement>("input"),
    toasts: byId("toasts"),
  });
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
}

main().catch((e) => {
  console.error(e);
  document.body.textContent = `tersh failed to start: ${e instanceof Error ? e.message : String(e)}`;
  document.body.classList.add("fatal");
  void backend.windowReady().catch(() => undefined);
});
