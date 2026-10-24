import { describe, expect, it } from "vitest";
import { normalizeCombo } from "../plugins/api";
import { applySettings, cssVariables, isLight, resolveKeybindings } from "../settings";
import type { Config, Settings } from "../types";

const config = (overrides: Partial<Config> = {}): Config => ({
  shell: null,
  args: [],
  env: {},
  scrollback: 1000,
  font: { family: "monospace", size: 14, line_height: 1.2, bold_is_bright: false },
  window: { padding: 10, opacity: 0.9, blur: true, width: 800, height: 600 },
  cursor: { shape: "block", blink: false },
  theme: {
    name: "test",
    foreground: "#d4d4d4",
    background: "#1e1e1e",
    cursor: "#ffffff",
    selection: "#264f78",
    accent: "#3b8eea",
    palette: Array.from({ length: 16 }, (_, i) => `#0000${i.toString(16).padStart(2, "0")}`),
  },
  keybindings: {},
  rich: { enabled: true, osc: true, html_network: false },
  plugins: { disabled: [] },
  warnings: [],
  ...overrides,
});

describe("settings", () => {
  it("exposes the theme as CSS variables", () => {
    const vars = cssVariables(config());
    expect(vars["--bg"]).toBe("rgba(30, 30, 30, 0.9)");
    expect(vars["--accent"]).toBe("#3b8eea");
    expect(vars["--ansi-15"]).toBe("#00000f");
  });

  it("detects light themes", () => {
    expect(isLight("#ffffff")).toBe(true);
    expect(isLight("#eff1f5")).toBe(true);
    expect(isLight("#1e1e2e")).toBe(false);
  });

  it("applies variables and the user stylesheet, replacing it on reload", () => {
    const settings: Settings = { config: config(), style: "#cursor { color: red }", config_path: "/c", themes: [] };
    applySettings(settings);
    applySettings({ ...settings, style: "body { margin: 1px }" });
    const root = document.documentElement;
    expect(root.style.getPropertyValue("--fg")).toBe("#d4d4d4");
    expect(root.dataset.themeKind).toBe("dark");
    const styles = document.querySelectorAll("#user-style");
    expect(styles.length).toBe(1);
    expect(styles[0].textContent).toBe("body { margin: 1px }");
  });

  it("merges user keybindings over the defaults", () => {
    const table = resolveKeybindings({ "Ctrl+Shift+K": "none", "alt+b": { send: "\x1bb" }, "ctrl+shift+c": "paste" }, false);
    expect(table.has("ctrl+shift+k")).toBe(false);
    expect(table.get("alt+b")).toEqual({ send: "\x1bb" });
    expect(table.get("ctrl+shift+c")).toBe("paste");
    expect(table.get("ctrl+=")).toBe("zoom_in");
    expect(resolveKeybindings({}, true).get("meta+c")).toBe("copy");
  });

  it("accepts a literal plus key", () => {
    expect(normalizeCombo("ctrl++")).toBe("ctrl++");
    expect(normalizeCombo("ctrl+plus")).toBe("ctrl++");
    expect(normalizeCombo("Ctrl+Shift++")).toBe("ctrl+shift++");
    expect(normalizeCombo("+")).toBe("+");
  });
});
