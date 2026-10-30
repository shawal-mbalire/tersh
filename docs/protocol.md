# Rich card protocol

Programs ask tersh to show cards with JSON messages. There are two ways to
send them:

- **Escape sequence:** write `ESC ] 7701 ; PAYLOAD BEL` (or end with `ESC \`)
  to the terminal. `PAYLOAD` is a JSON object or its base64 encoding. tersh
  strips the sequence before the emulator sees it. Inside tmux, wrap it for
  passthrough: `ESC P tmux; <sequence with every ESC doubled> ESC \`.
- **Socket:** connect to the Unix socket in `$TERSH_SOCKET` and write one JSON
  object per line. Each line gets a one-line reply: `{"ok":true}` or
  `{"ok":false,"error":"..."}`. Use the socket for large payloads, or when you
  must know a card is placed before you print more. The socket only
  accepts the user's own connections (mode `0600`).

Programs that can't reach the socket (a web server in a container, a language
without Unix sockets) can use HTTP through `tersh-show --serve`. See
[building-on-tersh.md](building-on-tersh.md#web-servers-and-other-languages).

Setting `rich.osc = false` makes tersh accept cards only from the socket. Then
`cat`-ing an untrusted file can't create cards.

## Messages

```json
{
  "op": "show",
  "id": "plot-1",
  "kind": "image",
  "title": "loss",
  "data": "data:image/png;base64,...",
  "anchor": { "mode": "cursor", "rows": 12 }
}
```

| Field    | Meaning                                                                                                                    |
| -------- | -------------------------------------------------------------------------------------------------------------------------- |
| `op`     | `show` (default), `update`, `move`, `close`, `clear`, `info` (socket only: returns the grid size and cursor)               |
| `id`     | Card id. `show` with an existing id replaces that card. Omit it for a generated id.                                        |
| `kind`   | Renderer: `text`, `json`, `markdown`, `image`, `video`, `svg`, `html`, `plotly`, `notebook`, `output`, or a plugin's kind. |
| `data`   | Content (see below).                                                                                                       |
| `title`  | Title shown in the card's header.                                                                                          |
| `chrome` | `false` hides the header and frame.                                                                                        |
| `hidden` | `true` keeps the card but doesn't draw it (e.g. its line scrolled out of an editor window).                                |
| `anchor` | Where the card goes (see below). `update` and `move` without `anchor` keep the current one.                                |
| `screen` | For `clear`: only clear `"primary"` or `"alt"` cards.                                                                      |

`update` replaces `data` (and any other fields given). It creates the card if
it doesn't exist and `kind` is given. `move` changes only `anchor` and `hidden`.

### Anchors

| Mode               | Fields                                                                                 | Behaviour                                                                                                         |
| ------------------ | -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `cursor` (default) | `rows`, `cols`                                                                         | Starts at the cursor. On the normal screen it scrolls with the output, and is removed once it leaves the history. |
| `cell`             | `row`, `col` (screen cells, 0-based), `rows`, `cols`                                   | A fixed screen position. On the alternate screen (Neovim, tmux) this is what editor integrations use.             |
| `float`            | `top`, `left`, `width`, `height`: numbers are cells, strings are CSS lengths (`"10%"`) | Over the whole terminal, not tied to text.                                                                        |

If `rows` is omitted, the card sizes itself to its content. Cards anchored on
the alternate screen are removed when the program leaves it.

### Data by kind

| Kind       | `data`                                                                                             |
| ---------- | -------------------------------------------------------------------------------------------------- |
| `text`     | string                                                                                             |
| `markdown` | string; `$...$` and `$$...$$` are rendered with KaTeX                                              |
| `json`     | any JSON value (or a string holding JSON)                                                          |
| `image`    | `data:` URI, or base64 with a `mime` field                                                         |
| `video`    | like `image`; plays muted and looping. `autoplay`, `loop`, `muted`, `controls` (booleans) override |
| `svg`      | SVG markup (sanitised)                                                                             |
| `html`     | HTML document; runs in a sandboxed iframe with no network unless `rich.html_network = true`        |
| `plotly`   | `{ "data": [...], "layout": {...} }` (object or JSON string)                                       |
| `notebook` | an nbformat 4 notebook (object or JSON string)                                                     |
| `output`   | one Jupyter output, a list of them, or a bare MIME bundle                                          |

## Shell example

```sh
card() {  # card KIND FILE
  printf '\033]7701;%s\a' "$(jq -cn --arg k "$1" --rawfile d "$2" '{kind:$k, data:$d, anchor:{rows:10}}' | base64 -w0)"
  printf '\n%.0s' $(seq 10)   # leave room below the prompt
}
```

`tersh-show` (in `crates/tersh-show`) does this properly, including kind
detection, images, video and tmux. `tersh-show --pane FILE` covers the current
tmux pane with the card.

Video plays through the system's media stack. On Linux that is GStreamer: WebM
(VP8/VP9) works with the usual `gst-plugins-good`; MP4/H.264 needs the libav
plugin (`gstreamer1-plugin-libav` on Fedora).
