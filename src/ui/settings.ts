// Applies user settings to the page: theme colours as CSS variables and the
// user's style.css.

import { cssVariables, isLight } from "../domain/theme";
import type { Settings } from "../domain/types";

export function applySettings(settings: Settings, root: HTMLElement = document.documentElement) {
  const { config } = settings;
  for (const [name, value] of Object.entries(cssVariables(config))) root.style.setProperty(name, value);
  const light = isLight(config.theme.background);
  root.style.colorScheme = light ? "light" : "dark";
  root.dataset.theme = config.theme.name;
  root.dataset.themeKind = light ? "light" : "dark";

  // The user stylesheet goes last so it wins over built-in rules of equal specificity.
  let style = document.getElementById("user-style") as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = "user-style";
  }
  style.textContent = settings.style ?? "";
  document.head.append(style);
}
