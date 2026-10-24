// An action that toggles a solid, distraction-free background and bigger padding.
// Install: copy to ~/.config/tersh/plugins/focus-mode.js, then bind it:
//   [keybindings]
//   "ctrl+shift+f" = "focus.toggle"

export function activate(tersh) {
  let style = null;

  tersh.registerAction("focus.toggle", () => {
    if (style) {
      style.dispose();
      style = null;
      tersh.notify("focus mode off");
      return;
    }
    const pad = tersh.config.window.padding * 3;
    style = tersh.addStyle(`
      :root { --bg: var(--bg-solid) !important; --padding: ${pad}px !important; }
      #cards { opacity: 0.35; }
    `);
    tersh.notify("focus mode on");
  });

  // Turn it off again if the config reloads, so the new padding applies.
  tersh.on("config", () => {
    style?.dispose();
    style = null;
  });
}
