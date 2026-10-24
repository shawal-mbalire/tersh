# Plugins

A plugin is JavaScript that runs in the terminal window. It can add card
renderers, actions, keybindings and styles, react to terminal events, and type
into the shell.

Put it in `~/.config/tersh/plugins/` as either:

- `NAME.js`, or
- `NAME/index.js`, with an optional `NAME/style.css` loaded alongside it.

Plugins load in name order once the shell has started. They hot-reload: when
a plugin file changes, every plugin is unloaded (each one's renderers,
actions, keybindings, listeners and styles are undone, and the `Disposable`
or function its `activate` returned is called) and the current versions are
loaded. Disable one without
deleting it with `[plugins] disabled = ["NAME"]` in `config.toml`.

> Plugins have the same privileges as the terminal window, which includes
> sending input to your shell. Only install plugins you trust.

## Shape

A plugin is an ES module exporting `activate(tersh)` (or a default function):

```js
// ~/.config/tersh/plugins/hello.js
export function activate(tersh) {
  // A new card kind: anything sending {"kind": "greeting"} uses it.
  tersh.registerRenderer("greeting", ({ el, data }) => {
    el.innerHTML = `<p style="padding: 8px; color: var(--accent)">Hello, ${data}!</p>`;
  });

  // An action users can bind in [keybindings], e.g. "ctrl+shift+h" = "hello.wave".
  tersh.registerAction("hello.wave", () => {
    tersh.cards.show({ kind: "greeting", data: "world", anchor: { mode: "float", top: 2, left: 4, width: 30 } });
  });

  tersh.on("config", (config) => tersh.notify(`theme is now ${config.theme.name}`));
}
```

`examples/plugins/` has more complete examples.

## API

| Member                               | Description                                                                                                                                    |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `version`                            | tersh version.                                                                                                                                 |
| `config`                             | The current config (read-only). It is replaced on reload, so read it each time rather than keeping a copy.                                     |
| `registerRenderer(kind, renderer)`   | Render cards of `kind`, replacing any existing renderer, including built-in ones.                                                              |
| `registerAction(name, handler)`      | Add a bindable action. Return `false` from the handler to pass the key on to the program.                                                      |
| `runAction(name)`                    | Run any action (`copy`, `zoom_in`, ...).                                                                                                       |
| `actions()`                          | All action names.                                                                                                                              |
| `registerKeybinding(combo, handler)` | Handle a key before config bindings and the program see it. Return `false` to pass it on. Prefer `registerAction` so users can choose the key. |
| `onRich(handler)`                    | See every rich message first; return `true` to consume it (e.g. to add your own `op`s).                                                        |
| `on(event, handler)`                 | Events: `frame`, `config`, `title`, `bell`, `resize`, `focus`, `exit`.                                                                         |
| `write(text)` / `paste(text)`        | Send input to the shell. `paste` respects bracketed paste mode.                                                                                |
| `cards.show/update/close/clear/list` | Manage cards directly (same messages as [the protocol](protocol.md)).                                                                          |
| `grid()`                             | Columns, rows, cell size in pixels, cursor, alternate-screen flag, `rowText(y)`.                                                               |
| `notify(message, level?)`            | Show a toast (`"info"` or `"error"`).                                                                                                          |
| `addStyle(css)`                      | Add a stylesheet.                                                                                                                              |

Every `register*`, `on*` and `addStyle` call returns `{ dispose() }`.

### Renderers

```ts
type CardRenderer = (ctx: {
  id: string;
  kind: string;
  data: unknown;
  message: RichMessage; // the whole message, including your own extra fields
  el: HTMLElement; // render into this
  relayout(): void; // call after the content changes size
}) => void | { update?(data, message): void; dispose?(): void } | Promise<...>;
```

If you return `update`, new data is passed to it. Otherwise the card is
rendered again from scratch. `dispose` runs when the card closes.

Style cards with the theme variables (`--fg`, `--accent`, `--ansi-N`,
`--card-bg`, ...) so they follow the user's theme.
