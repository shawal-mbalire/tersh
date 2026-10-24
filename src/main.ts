import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { loadPlugins } from "./plugins/loader";
import { applySettings } from "./settings";
import { Terminal } from "./terminal";
import type { Settings } from "./types";

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/** Show config problems without burying the terminal in toasts. */
function reportWarnings(terminal: Terminal, settings: Settings) {
  const { warnings } = settings.config;
  for (const warning of warnings.slice(0, 3)) terminal.notify(warning, "error");
  if (warnings.length > 3) terminal.notify(`…and ${warnings.length - 3} more problems in ${settings.config_path}`, "error");
}

const loadFont = (settings: Settings) => {
  const { family, size } = settings.config.font;
  // Measure with the real font, not a fallback.
  return document.fonts.load(`${size}px ${family}`).catch(() => undefined);
};

async function main() {
  const settings = await invoke<Settings>("get_settings");
  applySettings(settings);
  await loadFont(settings);
  await document.fonts.ready;

  const terminal = new Terminal(settings.config, {
    terminal: byId("terminal"),
    canvas: byId<HTMLCanvasElement>("grid"),
    cursor: byId("cursor"),
    selection: byId("selection"),
    cards: byId("cards"),
    input: byId<HTMLTextAreaElement>("input"),
    toasts: byId("toasts"),
  });
  await terminal.start();
  (window as unknown as { tersh: unknown }).tersh = terminal.api;
  reportWarnings(terminal, settings);

  await listen<Settings>("settings-changed", async ({ payload }) => {
    applySettings(payload);
    await loadFont(payload);
    terminal.reconfigure(payload.config);
    if (payload.config.warnings.length) reportWarnings(terminal, payload);
    else terminal.notify(`config reloaded · theme ${payload.config.theme.name}`);
  });

  await loadPlugins(terminal.api);
}

main().catch((e) => {
  console.error(e);
  document.body.textContent = `tersh failed to start: ${e instanceof Error ? e.message : String(e)}`;
  document.body.classList.add("fatal");
});
