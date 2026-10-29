// Composition root: picks the Tauri backend and wires it into the terminal UI.

import "@fontsource-variable/sono/wght.css";
import { TauriBackend } from "./adapters/tauri/backend";
import { setCardDocuments } from "./ui/cards/documents";
import { loadFonts } from "./ui/fonts";
import { PluginHost } from "./ui/plugins/loader";
import { applySettings } from "./ui/settings";
import { Terminal } from "./ui/terminal";
import type { Settings } from "./domain/types";

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/** Show config problems without burying the terminal in toasts. */
function reportWarnings(terminal: Terminal, settings: Settings) {
  const { warnings } = settings.config;
  for (const warning of warnings.slice(0, 3)) terminal.notify(warning, "error");
  if (warnings.length > 3) terminal.notify(`…and ${warnings.length - 3} more problems in ${settings.config_path}`, "error");
}

async function main() {
  const backend = new TauriBackend();
  setCardDocuments(backend.cardDocuments);
  const settings = await backend.getSettings();
  applySettings(settings);

  const terminal = new Terminal(settings.config, backend, {
    terminal: byId("terminal"),
    canvas: byId<HTMLCanvasElement>("grid"),
    cursor: byId("cursor"),
    selection: byId("selection"),
    cards: byId("cards"),
    input: byId<HTMLTextAreaElement>("input"),
    toasts: byId("toasts"),
  });
  // Start the shell and load fonts concurrently so the shell initializes while
  // WebKit finishes rasterising the bundled font faces.
  const fontsReady = loadFonts(settings.config.font).then(() => document.fonts.ready);
  await Promise.all([terminal.start(), fontsReady]);
  terminal.refreshFonts();
  (window as unknown as { tersh: unknown }).tersh = terminal.api;
  reportWarnings(terminal, settings);

  const plugins = new PluginHost(terminal.api, backend);
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
});
