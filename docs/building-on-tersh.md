# Building on tersh

tersh's rich cards are open to other programs at every level. Pick the one
that fits:

| You are writing...                         | Use                                                                  |
| ------------------------------------------ | -------------------------------------------------------------------- |
| a CLI or script running inside tersh       | `tersh-show FILE`, or the [protocol](protocol.md) directly           |
| a Neovim plugin                            | the [Lua API](#neovim-plugins)                                       |
| a web server, or anything that speaks HTTP | the [HTTP bridge](#web-servers-and-other-languages)                  |
| a web page that should render like tersh   | [`@tersh/cards`](#rendering-cards-in-your-own-page)                  |
| a tersh extension                          | a [JavaScript plugin](plugins.md) with its own card kinds or actions |

## Neovim plugins

With tersh.nvim installed (see [neovim.md](neovim.md)), other plugins can
show cards without knowing anything about the terminal:

```lua
local ok, tersh = pcall(require, "tersh")
if ok and tersh.available() then
  -- Pinned under line 12: follows scrolling, edits, folds, splits and tmux panes.
  local id = tersh.attach({ kind = "output", data = jupyter_outputs, line = 12, rows = 15, title = "Out[3]" })
  tersh.update(id, new_outputs)

  -- Covering a window, e.g. a preview split.
  tersh.attach_window(win, { kind = "markdown", data = text })

  -- Any raw protocol message, e.g. a floating card. Not tied to a buffer.
  tersh.send({ kind = "video", data = "data:video/webm;base64,...", anchor = { mode = "float", top = 2, left = 4, width = 60, height = 20 } })
end
```

Plugins that want to work without tersh too should check `tersh.available()`
and fall back to text.

## Web servers and other languages

`tersh-show --serve` turns a tersh window into an HTTP endpoint:

```sh
tersh-show --serve                      # 127.0.0.1:7701, prints a random token
tersh-show --serve 127.0.0.1:9000 --token "$MY_TOKEN"
```

`POST /cards` takes one [protocol](protocol.md) message, or an array of them,
with `Authorization: Bearer TOKEN`:

```sh
curl -H "Authorization: Bearer $TOKEN" \
  -d '{"id":"build","kind":"markdown","data":"# Build passed","anchor":{"mode":"float","top":1,"left":2,"width":40}}' \
  http://127.0.0.1:7701/cards
```

```python
# e.g. from a Flask/FastAPI app, a test runner or a notebook kernel
requests.post("http://127.0.0.1:7701/cards", headers={"Authorization": f"Bearer {token}"},
              json={"id": "loss", "kind": "plotly", "data": figure.to_dict(), "anchor": {"mode": "float", "top": 2, "left": 2, "width": 80, "height": 24}})
```

Replies are `{"ok":true,"sent":N}`, or `{"ok":false,"error":"..."}` with a 4xx
status for bad requests, or 502 when tersh rejected a message. The bridge
answers CORS preflights, so a browser page can post to it too. The token is
what keeps other pages and users out. Use `float` anchors, because the bridge's
own cursor doesn't move.

## Rendering cards in your own page

[`packages/cards`](../packages/cards) is the code tersh renders cards with,
as a standalone package: Jupyter notebooks and outputs, markdown with KaTeX,
Plotly, sandboxed HTML, SVG, images, video, JSON and text.

```ts
import { mountCard } from "@tersh/cards";
import "@tersh/cards/styles.css";

const card = await mountCard(document.getElementById("out")!, { kind: "notebook", data: notebookJson });
await card.update({ data: newNotebookJson });
card.dispose();
```

A server can render notebooks for the browser this way, and a dev tool can
show the same previews in a web UI and in the terminal. See the
[package README](../packages/cards/README.md).
