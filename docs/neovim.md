# tersh.nvim

The plugin in [`editors/nvim`](../editors/nvim) draws tersh cards in Neovim:

- **Line cards** sit below a buffer line. They reserve blank virtual lines,
  so they never cover text, and follow the line through scrolling, inserted
  or deleted lines, wrapping, folds (hidden while folded), splits and tabs.
  Inside tmux they are offset to the pane.
- **Window cards** cover a window. `:TershPreview` uses one to render a
  file: markdown in a split next to its source, everything else (notebooks,
  images, video, SVG, HTML, Plotly JSON) over the file's own window.

Nothing happens outside tersh. The plugin talks to `$TERSH_SOCKET` and falls
back to escape sequences.

## Install

```lua
-- lazy.nvim: straight from the tersh repository
{ "shawal-mbalire/tersh", main = "tersh", lazy = false, opts = {} }
-- a local checkout
{ dir = "~/src/tersh", name = "tersh", main = "tersh", lazy = false, opts = {} }
-- other managers: add editors/nvim to 'runtimepath' and call require("tersh").setup()
```

Keep `lazy = false`. The plugin is small, and it has to be loaded at startup to
render files as they open. `:checkhealth tersh` checks the connection to
tersh and the tmux settings.

Neovim 0.10 or newer.

## Options

```lua
require("tersh").setup({
  auto_preview = { "ipynb", "md", "markdown", "svg", "html", "htm",   -- extensions rendered when opened;
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "avif",              -- {} to disable
    "webm", "mp4", "m4v", "ogv", "mov" },
  side_by_side = { "markdown" },                    -- kinds shown in a split next to the source;
                                                    -- other kinds are drawn over the file's window
  notebook = true,                                  -- open .ipynb as cells (see Notebooks)
  preview = { split = "vertical", size = 0.5 },     -- "vertical", "horizontal" or "tab"; size < 1 is a fraction
  rows = 12,                                        -- default height of :TershShow cards
  debounce = 150,                                   -- ms between an edit and the preview update
  force = false,                                    -- send even if tersh isn't detected
})
```

## Notebooks

`.ipynb` files open as cells, laid out like Jupyter:

- **Code cells** are shaded blocks of ordinary lines, with the notebook's
  language as the filetype, so highlighting, LSP and your keymaps work. An
  `In [n]:` prompt sits in the gutter, and saved outputs (text, tables,
  images, Plotly, errors) are drawn below with `Out[n]:` beside results.
- **Markdown cells** are drawn rendered, with LaTeX.
- A bar in the gutter marks the selected cell: blue in command mode, green
  while editing, as in Jupyter.
- Each cell starts with a `# %%` line (`# %% [markdown]`, `# %% [raw]`). It
  shows as the blank line between cells and appears only under the cursor.
- Cards step aside while a floating window or the completion menu would be
  under them.
- `:w` writes nbformat JSON the way Jupyter does (sorted keys, one-space
  indent) and keeps outputs, execution counts and metadata.
  `:TershPreview` renders the whole notebook over the window.

### Keys

Normal mode is Jupyter's edit mode: Vim works as usual. **Esc** in Normal
mode enters command mode (`-- NOTEBOOK --`), where single keys act on cells:

| Key             | Action                                           |
| --------------- | ------------------------------------------------ |
| `j` / `k`       | select the next / previous cell                  |
| `Enter`, `i`    | edit the cell (a markdown cell opens its source) |
| `a` / `b`       | insert a code cell above / below                 |
| `dd`            | delete the cell                                  |
| `z`             | undo                                             |
| `x` / `c`       | cut / copy the cell                              |
| `v` / `V`       | paste below / above                              |
| `m` / `y` / `r` | make the cell markdown / code / raw              |
| `M`             | merge with the cell below                        |
| `o`             | hide or show the cell's outputs                  |
| `s`             | save                                             |
| `Shift-Enter`   | run, select the next cell                        |
| `Ctrl-Enter`    | run                                              |
| `Alt-Enter`     | run, insert a cell below                         |
| `ii` / `00`     | interrupt / restart the kernel                   |
| `h`             | list these keys                                  |

Insert mode, or leaving the buffer, ends command mode. Editing a markdown
cell opens its source in a split below; the rendered cell follows as you
type, and `q` closes the split.

Set `notebook = false` to open notebooks as JSON with a rendered preview
instead.

### Running cells

`Shift-Enter` runs the cell and selects the next one (adding a cell at the
end), `Ctrl-Enter` runs it and stays, `Alt-Enter` runs it and inserts a cell
below. They work in command mode and while editing. Outputs stream in as the
kernel produces them, the prompt shows `In [*]:` while a cell runs, and the
winbar shows the kernel and whether it is busy. In command mode `ii`
interrupts the kernel and `00` restarts it. `:TershRun` runs the cell under
the cursor, `:TershRun!` every cell; `:TershInterrupt` and `:TershRestart`
do what they say.

The kernel starts on the first run and stops when the buffer is closed or
Neovim exits. tersh runs a small bridge,
[`python/tersh_kernel.py`](../editors/nvim/python/tersh_kernel.py), with a
Python that has `jupyter_client`, and picks the kernel like this:

1. the kernelspec named in the notebook's metadata, if it is installed;
2. `kernel.argv` from your options;
3. `python -m ipykernel_launcher` with the first of these that has
   `ipykernel`: `kernel.python`, the nearest `.venv`, `$VIRTUAL_ENV`,
   `$CONDA_PREFIX`, `python3`;
4. `uv run --with ipykernel` in the notebook's directory, which uses the
   project's dependencies in a uv project.

The bridge's Python is chosen from the same list (it needs `jupyter_client`),
falling back to `uv run --with jupyter_client`. With uv installed nothing else
is needed. Otherwise install `jupyter_client` and `ipykernel` into one of
those Pythons. The kernel uses local IPC sockets, not TCP.

```lua
require("tersh").setup({
  kernel = {
    python = "~/venvs/data/bin/python",                       -- default: found as above
    argv = { "python3", "-m", "ipykernel_launcher", "-f", "{connection_file}" },
  },
})
```

Kernels can't ask for input: `input()` raises an error.

## Commands

- `:[N]TershShow [file]` shows a file below the cursor line, `N` rows tall.
  Without an argument it uses the file name under the cursor, so you can put
  the cursor on `![](plot.png)` and run `:TershShow`.
- `:TershPreview` toggles a live preview of the current buffer. It renders
  markdown with LaTeX, HTML, SVG, Plotly JSON, notebooks, images and video.
  Markdown opens in a split beside the source, which closes when the source
  window switches to another file. Other kinds cover the file's window, so
  toggling shows the source again; the render hides while the window shows
  another buffer and comes back with it. While JSON is
  half-edited, the last valid render stays up.
- `:TershClose` closes the cards on the cursor line. `:TershClose!` closes
  all of them.

## Lua API

```lua
local tersh = require("tersh")

-- A card under line 42 of the current buffer, 15 rows tall.
local id = tersh.attach({ kind = "output", data = jupyter_outputs, line = 42, rows = 15, title = "Out[3]" })

tersh.update(id, new_outputs)     -- replace its data
tersh.close(id)

tersh.attach_window(win, { kind = "markdown", data = text })   -- cover a window
tersh.available()                 -- true inside tersh (or with `force`)
tersh.send(message, callback)     -- any raw protocol message, e.g. a float card; not tracked
tersh.show_file("~/plots/loss.png", 20)
tersh.cards_at(buf, line)         -- ids of cards on a line
tersh.sync(true)                  -- force a reposition (after a custom layout change)
```

`attach` options: `kind`, `data`, `title`, `buf`, `line` (1-based, default the
cursor line), `rows`, `id`, `chrome`, `mime`. Card kinds and data formats are
listed in [protocol.md](protocol.md). Use `kind = "output"` to show Jupyter
outputs, for example from a kernel integration like molten or a custom REPL.

## tmux

```tmux
set -g allow-passthrough on
set -g focus-events on                       # cards hide when their pane's window is in the background
set -ga update-environment " TERSH_SOCKET"
```
