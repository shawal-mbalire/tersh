// The Tauri implementation of the Backend port: commands and channels from
// src-tauri, the clipboard plugin and the window API.

import { Channel, convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { RichMessage } from "../../domain/cards";
import type { Frame, SessionInfo, Settings, TermEvent } from "../../domain/types";
import type { Backend, PluginSource, SessionHandlers } from "../../ports/backend";

export class TauriBackend implements Backend {
  startSession(cols: number, rows: number, handlers: SessionHandlers): Promise<SessionInfo> {
    const frames = new Channel<Frame>();
    frames.onmessage = (frame) => handlers.frame(frame);
    const rich = new Channel<RichMessage>();
    rich.onmessage = (message) => handlers.rich(message);
    const events = new Channel<TermEvent>();
    events.onmessage = (event) => handlers.event(event);
    return invoke<SessionInfo>("session_start", { cols, rows, frames, rich, events });
  }

  write(data: string) {
    return invoke<void>("session_write", { data });
  }

  resize(cols: number, rows: number, cellWidth: number, cellHeight: number) {
    return invoke<void>("session_resize", { cols, rows, cellWidth, cellHeight });
  }

  scroll(lines: number) {
    return invoke<void>("session_scroll", { lines });
  }

  scrollToBottom() {
    return invoke<void>("session_scroll_to_bottom");
  }

  getSettings() {
    return invoke<Settings>("get_settings");
  }

  async onSettingsChanged(handler: (settings: Settings) => void) {
    await listen<Settings>("settings-changed", ({ payload }) => handler(payload));
  }

  async reloadConfig() {
    await invoke("reload_config");
  }

  openConfig() {
    return invoke<void>("open_config");
  }

  listPlugins() {
    return invoke<PluginSource[]>("list_plugins");
  }

  readClipboard() {
    return readText().catch(() => null);
  }

  writeClipboard(text: string) {
    return writeText(text);
  }

  setTitle(title: string) {
    return getCurrentWindow().setTitle(title);
  }

  closeWindow() {
    return getCurrentWindow().close();
  }

  async onAppUpdated(handler: (version: string) => void) {
    await listen<string>("app-updated", ({ payload }) => handler(payload));
  }

  restart() {
    return invoke<void>("restart_app");
  }

  cardDocuments = {
    put: (id: string, html: string) => invoke<void>("card_html_put", { id, html }),
    drop: (id: string) => void invoke("card_html_drop", { id }),
    url: (id: string) => convertFileSrc(id, "tersh-card"),
  };
}
