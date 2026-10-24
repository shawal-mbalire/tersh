# tersh

A small, fast terminal that can show rich content. You get images, markdown
with LaTeX, Plotly charts, SVG, HTML and whole Jupyter notebooks, drawn as
cards over the text grid and kept in step with it. tersh has no tabs or
splits; use tmux for those.

- **Fast core:** Rust PTY host (`portable-pty` and `alacritty_terminal`) with
  a canvas grid renderer in a Tauri webview.
- **Rich cards:** programs send JSON over an escape sequence or a local
  socket, and tersh renders it as a card over the cells it names.
- **Your terminal, your way:** a commented config with themes, keybindings,
  your own CSS and JavaScript plugins. Changes apply when you save.
- **Editor-aware:** a Neovim plugin pins cards to buffer lines and keeps them
  in place as you scroll, edit and split. It also works inside tmux.

## Install

macOS and Linux (x86_64 and arm64):

```sh
curl -fsSL https://raw.githubusercontent.com/shawal-mbalire/tersh/main/install.sh | sh
```

This installs the app and `tersh-show` into `~/.local/bin` and adds a desktop
entry on Linux. Options go after `sh -s --`:

```sh
curl -fsSL https://raw.githubusercontent.com/shawal-mbalire/tersh/main/install.sh | sh -s -- --nvim     # + Neovim plugin
curl -fsSL https://raw.githubusercontent.com/shawal-mbalire/tersh/main/install.sh | sh -s -- --no-app   # tersh-show only (e.g. over ssh)
```

On Windows, download the `.msi` from
[Releases](https://github.com/shawal-mbalire/tersh/releases).

### Updating

```sh
tersh update            # install the latest release
tersh update --check    # only check
```

Open tersh windows notice the new version and restart into it, keeping the
same directory and window size:

- **Shell idle at its prompt:** restarts straight away.
- **tmux or zellij running:** restarts straight away and re-attaches, since
  your sessions keep running.
- **Anything else running (an editor, a build):** waits until the shell is
  idle again, with a notification. The `restart` action
  (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F5</kbd>) restarts immediately.

Set `[update] auto_restart = false` to only be notified.

## Building from source

```sh
npm install
npm run tauri dev        # or: npm run tauri build
```

Linux needs the WebKitGTK development packages: `webkit2gtk4.1-devel` and
`libsoup3-devel` on Fedora, or `libwebkit2gtk-4.1-dev` on Debian/Ubuntu.
Install `tersh-show` with `cargo install --path crates/tersh-show`.

## Configuration

On first run tersh writes `~/.config/tersh/config.toml`, a fully commented
template listing every option. To see or edit it, press
<kbd>Ctrl</kbd>+<kbd>,</kbd> in tersh (<kbd>Cmd</kbd>+<kbd>,</kbd> on macOS),
or open it directly:

```sh
$EDITOR ~/.config/tersh/config.toml      # macOS: ~/Library/Application Support/tersh/config.toml
```

Every key is optional. Everything hot-reloads, so there's nothing to restart:
saving `config.toml`, a theme, `style.css` or a plugin updates the open window
in place, and installing or removing a font is picked up too. Mistakes such
as a misspelled key, an unknown theme or a bad colour show up as a
notification instead of being silently ignored.

### Fonts

tersh bundles two fonts:

- **Sono:** the default text font.
- **Symbols Nerd Font Mono** (Nerd Fonts 3.4): used only for icon codepoints.

Nerd Font icons (Powerline, devicons, Material Design, codicons, ...) work
with any `font.family`, whether or not you have a Nerd Font installed, and
even if yours is an older v2 build that lacks the v3 icons. kitty does the
same thing.

```toml
shell = "/usr/bin/fish"
theme = "tokyo-night"

[font]
family = "'Sono Variable', monospace"           # Sono is bundled and the default
size = 13

[window]
padding = 12
opacity = 0.9

[cursor]
shape = "beam"
blink = true

[keybindings]
"ctrl+shift+k" = "none"                       # give the key back to programs
"alt+left" = { send = "\u001bb" }             # type text into the shell
"ctrl+shift+g" = { send = "git status\r" }
```

Colours, font, padding, opacity, cursor and keybindings change live. `shell`,
`args`, `env` and `scrollback` apply to new windows.

| File                               | Purpose                                                    |
| ---------------------------------- | ---------------------------------------------------------- |
| `~/.config/tersh/config.toml`      | settings                                                   |
| `~/.config/tersh/themes/NAME.toml` | your themes                                                |
| `~/.config/tersh/style.css`        | CSS over the built-in styles (cards, cursor, toasts, ...)  |
| `~/.config/tersh/plugins/`         | JavaScript plugins, see [docs/plugins.md](docs/plugins.md) |

### Themes

| Family      | Themes                                                                              |
| ----------- | ----------------------------------------------------------------------------------- |
| tersh       | `tersh` (default), `tersh-light`                                                    |
| Catppuccin  | `catppuccin-mocha`, `catppuccin-macchiato`, `catppuccin-frappe`, `catppuccin-latte` |
| Tokyo Night | `tokyo-night`, `tokyo-night-storm`, `tokyo-night-moon`, `tokyo-night-day`           |
| Rosé Pine   | `rose-pine`, `rose-pine-moon`, `rose-pine-dawn`                                     |
| Gruvbox     | `gruvbox-dark`, `gruvbox-light`                                                     |
| Solarized   | `solarized-dark`, `solarized-light`                                                 |
| GitHub      | `github-dark`, `github-light`                                                       |
| Others      | `nord`, `dracula`, `one-dark`, `kanagawa`, `everforest-dark`                        |

Names are forgiving: `tokyonight`, `TokyoNight` and `tokyo-night` are the same
theme, and a family name such as `catppuccin`, `gruvbox` or `rosepine` picks
its usual variant.

```toml
theme = "catppuccin-macchiato"
```

To adjust a theme, override some of its keys:

```toml
[theme]
name = "nord"
background = "#20242c"
palette = ["#3b4252", "#ff6b6b"]   # replaces ANSI 0 and 1 only
```

To make your own theme, create `themes/NAME.toml` with the same keys. Its
`name` key is the theme it extends. A file named after a built-in theme
restyles that theme.

```toml
# ~/.config/tersh/themes/midnight.toml
name = "tokyo-night"
background = "#0b0c10"
accent = "#ff9e64"
```

Cards, notebooks and ANSI-coloured output use the theme as well. These CSS
variables are available to `style.css` and to plugins: `--bg`, `--bg-solid`,
`--fg`, `--accent`, `--cursor`, `--selection`, `--ansi-0` to `--ansi-15`,
`--card-bg`, `--card-border`, `--code-bg`, `--font-family` and `--font-size`.
`<html data-theme-kind="light|dark">` lets you style light and dark themes
separately.

```css
/* style.css */
.card {
  border-radius: 10px;
  border-color: var(--accent);
}
#cursor.shape-block {
  border-radius: 2px;
}
```

### Keybindings and actions

| Default (macOS: <kbd>Cmd</kbd>)                              | Action                                |
| ------------------------------------------------------------ | ------------------------------------- |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>C</kbd> / <kbd>V</kbd> | `copy` / `paste`                      |
| <kbd>Ctrl</kbd>+<kbd>=</kbd> / <kbd>-</kbd> / <kbd>0</kbd>   | `zoom_in` / `zoom_out` / `zoom_reset` |
| <kbd>Shift</kbd>+<kbd>PageUp</kbd> / <kbd>PageDown</kbd>     | `scroll_page_up` / `scroll_page_down` |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>K</kbd>                | `clear_cards`                         |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>R</kbd>                | `reload_config`                       |
| <kbd>Ctrl</kbd>+<kbd>,</kbd>                                 | `open_config`                         |

`scroll_top` and `scroll_bottom` are also available. Plugins can add actions
(for example `myplugin.toggle`) that you bind the same way. Combos are written
as `ctrl+alt+shift+meta+KEY`, using the key's name: `enter`, `escape`,
`pageup`, `f5`, `plus`, and so on.

## Rich cards

From the shell:

```sh
tersh-show plot.png                 # inline, below the prompt
tersh-show --rows 20 notes.md       # reserve 20 rows
tersh-show analysis.ipynb           # notebooks float over the terminal
tersh-show --float fig.json         # a Plotly figure
some-command | tersh-show -k markdown -
```

Any program can do the same by writing `ESC ] 7701 ; <json or base64 json> BEL`,
or by sending JSON lines to `$TERSH_SOCKET`. See
[docs/protocol.md](docs/protocol.md).

### Neovim

The plugin in `editors/nvim` attaches cards to buffer lines. They reserve
blank virtual lines and follow the line through scrolling, edits, folds,
splits and tmux panes. It also adds a live preview split for markdown, HTML,
SVG, Plotly JSON and notebooks.

```lua
-- lazy.nvim
{ dir = "~/src/tersh/editors/nvim", opts = {} }
```

| Command                | What it does                                                    |
| ---------------------- | --------------------------------------------------------------- |
| `:[N]TershShow [file]` | show a file (default: the one under the cursor) below this line |
| `:TershPreview`        | toggle a live rendered preview of the buffer                    |
| `:TershClose[!]`       | close cards on this line (`!`: all)                             |

Opening an `.ipynb` file opens its preview automatically. Turn this off with
`auto_preview = {}`. Other plugins can use the Lua API
(`require("tersh").attach{...}`, `update`, `close`, `attach_window`), for
example to show cell outputs under code. See [docs/neovim.md](docs/neovim.md).

### tmux

tersh relies on tmux for tabs, panes and sessions. Add this to `tmux.conf`:

```tmux
set -g allow-passthrough on                  # let card escape sequences through
set -g focus-events on                       # hide cards of panes in background windows
set -ga update-environment " TERSH_SOCKET"   # use the faster socket in new panes
```

## Development

The code uses a hexagonal (ports and adapters) layout: a pure domain
(`crates/tersh-domain`, `src/domain`), use cases and ports
(`crates/tersh-app`, `src/ports`), and adapters for the PTY, emulator, socket,
filesystem and Tauri (`crates/tersh-adapters`, `src/adapters`, `src-tauri`).
See [docs/architecture.md](docs/architecture.md).

```sh
npm test && npm run typecheck                          # frontend, including the layering check
cargo test                                             # domain, use cases (with fakes), adapters, tersh-show
nvim --headless -u NONE -l editors/nvim/tests/run.lua  # Neovim plugin
```
