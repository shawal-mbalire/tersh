# tersh.nvim

The plugin in [`editors/nvim`](../editors/nvim) draws tersh cards in Neovim:

- **Line cards** sit below a buffer line. They reserve blank virtual lines,
  so they never cover text, and follow the line through scrolling, inserted
  or deleted lines, wrapping, folds (hidden while folded), splits and tabs.
  Inside tmux they are offset to the pane.
- **Window cards** cover a window. `:TershPreview` uses one for its live
  preview split.

Nothing happens outside tersh. The plugin talks to `$TERSH_SOCKET` and falls
back to escape sequences.

## Install

```lua
-- lazy.nvim
{ dir = "~/src/tersh/editors/nvim", opts = {} }
-- or add the directory to 'runtimepath' and call require("tersh").setup()
```

Neovim 0.10 or newer.

## Options

```lua
require("tersh").setup({
  auto_preview = { "ipynb" },                       -- extensions previewed when opened; {} to disable
  preview = { split = "vertical", size = 0.5 },     -- "vertical", "horizontal" or "tab"; size < 1 is a fraction
  rows = 12,                                        -- default height of :TershShow cards
  debounce = 150,                                   -- ms between an edit and the preview update
  force = false,                                    -- send even if tersh isn't detected
})
```

## Commands

- `:[N]TershShow [file]` shows a file below the cursor line, `N` rows tall.
  Without an argument it uses the file name under the cursor, so you can put
  the cursor on `![](plot.png)` and run `:TershShow`.
- `:TershPreview` toggles a live preview of the current buffer. It renders
  markdown with LaTeX, HTML, SVG, Plotly JSON and notebooks. While JSON is
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
