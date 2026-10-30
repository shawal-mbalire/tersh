# Features

Everything tersh does, with a stable ID, how to check it, and its status on
each platform. This is the inventory the cross-platform test suite is built
from: each row is one test case. Automate a row when you can and link the test.
Until then, check it by hand before a release.

**Status:** ✅ verified (automated, or checked by hand for the release) ·
🧪 expected to work, not yet verified · ⚠️ known issue (see notes) ·
— not applicable.

**Automated by:** `vitest` = `npm test` (`src/__tests__`, `packages/cards/test`);
`cargo` = `cargo test` (crate named); `nvim` =
`editors/nvim/tests/run.lua`; `install.yml` = the installer workflow on all
three OSes; `ci desktop` = the Tauri build and clippy on Linux.

## Install and update (INST)

| ID      | Feature                                                                                                | How to check                                                                                      | Linux | macOS | Windows | Automated by                |
| ------- | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------- | :---: | :---: | :-----: | --------------------------- |
| INST-01 | One command installs everything: `curl … /install \| sh` or `irm … /install \| iex`                    | Run it on a clean account; app, `tersh-show`, Neovim plugin, completions and tmux block are there |  ✅   |  🧪   |   🧪    | install.yml                 |
| INST-02 | `tersh` and `tersh-show` are on PATH afterwards                                                        | New shell: `tersh --version`, `tersh-show --help`                                                 |  ✅   |  🧪   |   🧪    | install.yml                 |
| INST-03 | Desktop integration: launcher entry (Linux), `tersh.app` in Applications (macOS), Start menu (Windows) | Find "tersh" in the app launcher / Spotlight / Start                                              |  ✅   |  ⚠️   |   🧪    | install.yml (file present)  |
| INST-04 | Neovim plugin: lazy.nvim spec if lazy is used, package otherwise, skipped if already configured        | Check `lua/plugins/tersh.lua` or `site/pack/tersh`; rerun leaves an existing spec alone           |  ✅   |  🧪   |   🧪    | install.yml                 |
| INST-05 | tmux block added once, applied to a running server                                                     | `grep '>>> tersh >>>' tmux.conf` gives 1 after two runs; `tmux show -g allow-passthrough` is `on` |  ✅   |  🧪   |    —    | install.yml                 |
| INST-06 | carapace completions installed when carapace is used                                                   | `specs/tersh.yaml` exists; `tersh <TAB>` completes                                                |  ✅   |  🧪   |   🧪    |                             |
| INST-07 | Opt-outs `--no-app/--no-nvim/--no-tmux/--no-completions` (or `TERSH_NO_*`), remembered                 | Run with `--no-app`, then without flags: the app isn't reinstalled                                |  ✅   |  🧪   |   🧪    | install.yml                 |
| INST-08 | `TERSH_VERSION` / `--version` installs a specific release                                              | `TERSH_VERSION=v0.3.0`: `tersh --version` prints it                                               |  ✅   |  🧪   |   🧪    |                             |
| INST-09 | `tersh update --check` reports a newer release                                                         | With an older install: prints "vX is available"                                                   |  ✅   |  🧪   |   ⚠️    |                             |
| INST-10 | `tersh update` installs the latest release with the same components                                    | Run it; `tersh --version` shows the new version and the app still starts                          |  ✅   |  🧪   |   ⚠️    |                             |
| INST-11 | Open windows restart into the new version: right away at an idle prompt or in tmux, else when idle     | Update with a window at a prompt, one in tmux, one running `vim`                                  |  ✅   |  🧪   |   🧪    | cargo (tersh-domain update) |
| INST-12 | The installer never replaces the app's own binary (self-update keeps working)                          | Run `tersh update` from inside tersh twice; the app still starts                                  |  ✅   |   —   |    —    |                             |

## Launch and window (WIN)

| ID     | Feature                                                                          | How to check                                                                    | Linux | macOS | Windows | Automated by |
| ------ | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | :---: | :---: | :-----: | ------------ |
| WIN-01 | Launches from the OS launcher like a normal app                                  | Linux launcher / macOS Spotlight and Dock / Windows Start                       |  ✅   |  ⚠️   |   🧪    |              |
| WIN-02 | Launches from a shell: `tersh`, `tersh --cwd DIR --input TEXT --size WxH`        | The window opens in DIR at the given size, and TEXT is typed into the shell     |  ✅   |  🧪   |   🧪    | cargo (cli)  |
| WIN-03 | `tersh --version` / `--help` print to the console                                | Run from a terminal                                                             |  ✅   |  🧪   |   ⚠️    | cargo (cli)  |
| WIN-04 | Translucent background (`window.opacity`)                                        | `opacity = 0.7`: the desktop shows through                                      |  ✅   |  ⚠️   |   🧪    |              |
| WIN-05 | Backdrop blur (`window.blur`)                                                    | macOS vibrancy, Windows acrylic, Linux from the compositor (e.g. Hyprland blur) |  ✅   |  🧪   |   🧪    |              |
| WIN-06 | Frameless window: drag by the top padding, resize from the edges                 | Drag and resize                                                                 |  ✅   |  🧪   |   🧪    |              |
| WIN-07 | Starts in the home directory when launched from the GUI                          | Launch from the launcher; `pwd`                                                 |  ✅   |  🧪   |   🧪    |              |
| WIN-08 | The shell gets a full user environment (PATH, profile) when started from the GUI | Launch from the launcher; `echo $PATH` includes Homebrew, `~/.local/bin`, etc.  |  ✅   |  ⚠️   |   🧪    |              |
| WIN-09 | Window title follows the shell / program title                                   | `printf '\e]0;hello\a'`                                                         |  ✅   |  🧪   |   🧪    |              |

## Terminal (TERM)

| ID      | Feature                                                                                 | How to check                                                                      | Linux | macOS | Windows | Automated by               |
| ------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | :---: | :---: | :-----: | -------------------------- |
| TERM-01 | Runs `shell`/`args` from the config, or `$SHELL` / `%COMSPEC%`                          | Set `shell = "/bin/bash"`; open a new window                                      |  ✅   |  🧪   |   🧪    | cargo (tersh-adapters)     |
| TERM-02 | Sets `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=tersh`, `TERSH_SOCKET` | `env \| grep -E 'TERM\|TERSH'`                                                    |  ✅   |  🧪   |   🧪    | cargo (session_with_fakes) |
| TERM-03 | Shells don't inherit tmux's or another tersh's variables (`TMUX`, `TERSH_SOCKET`)       | Start tersh from inside tmux; `echo $TMUX` is empty                               |  ✅   |  🧪   |   🧪    | cargo (session_with_fakes) |
| TERM-04 | Shells don't inherit the AppImage's environment (`LD_LIBRARY_PATH`, `PYTHONHOME`, …)    | `env \| grep -i tersh/app` is empty; `grep x /dev/null` prints no library warning |  ✅   |   —   |    —    | cargo (tersh-domain env)   |
| TERM-05 | 16, 256 and 24-bit colours; bold, dim, italic, underline styles, strikethrough, inverse | `msgcat --color=test`, or a 24-bit colour test script                             |  ✅   |  🧪   |   🧪    |                            |
| TERM-06 | Alternate screen (vim, htop, less) with scrollback preserved                            | Open and quit `vim`; earlier output is intact                                     |  ✅   |  🧪   |   🧪    | cargo (tersh-adapters)     |
| TERM-07 | Scrollback (`scrollback` lines); mouse wheel and Shift+PageUp/PageDown                  | `seq 5000`, then scroll up                                                        |  ✅   |  🧪   |   🧪    |                            |
| TERM-08 | Mouse reporting for programs (tmux, vim `mouse=a`, htop)                                | Click and scroll in htop and in tmux panes                                        |  ✅   |  🧪   |   🧪    |                            |
| TERM-09 | Text selection and copy; middle-click paste                                             | Select with the mouse, copy, paste elsewhere                                      |  ✅   |  🧪   |   🧪    |                            |
| TERM-10 | Bracketed paste                                                                         | Paste multiple lines into bash/zsh: they aren't executed                          |  ✅   |  🧪   |   🧪    | vitest (input)             |
| TERM-11 | Focus in/out events                                                                     | vim `autoread`, tmux `focus-events`                                               |  ✅   |  🧪   |   🧪    |                            |
| TERM-12 | IME input (CJK, accents) with the candidate window at the cursor                        | Type with an input method                                                         |  🧪   |  🧪   |   🧪    |                            |
| TERM-13 | OSC 52 clipboard (e.g. from tmux or Neovim over ssh)                                    | `printf '\e]52;c;%s\a' "$(printf hi \| base64)"`, then paste                      |  ✅   |  🧪   |   🧪    |                            |
| TERM-14 | Bell flashes the window                                                                 | `printf '\a'`                                                                     |  ✅   |  🧪   |   🧪    |                            |
| TERM-15 | Resize reflows and resizes the PTY (`stty size` follows)                                | Resize the window; `stty size`                                                    |  ✅   |  🧪   |   🧪    |                            |
| TERM-16 | Keystroke latency: echo is never delayed; heavy output is coalesced (~240 Hz)           | Type while `yes` runs in another pane                                             |  ✅   |  🧪   |   🧪    |                            |

## Rendering (REN)

| ID     | Feature                                                                             | How to check                                                         | Linux | macOS | Windows | Automated by   |
| ------ | ----------------------------------------------------------------------------------- | -------------------------------------------------------------------- | :---: | :---: | :-----: | -------------- |
| REN-01 | Bundled Sono font (default) and any `font.family`                                   | Change `font.family`; it applies live                                |  ✅   |  🧪   |   🧪    | vitest (fonts) |
| REN-02 | Nerd Font icons with any font (bundled symbols)                                     | `echo '        '`                                                    |  ✅   |  🧪   |   🧪    | vitest (fonts) |
| REN-03 | Powerline separators and block elements fill the cell at any `line_height`          | tmux catppuccin status bar: rounded caps are solid; `printf '█▀▄▌▐'` |  ✅   |  🧪   |   🧪    |                |
| REN-04 | Zoom in, out and reset                                                              | Ctrl/Cmd + `=` / `-` / `0`                                           |  ✅   |  🧪   |   🧪    |                |
| REN-05 | Cursor shapes (block, beam, underline), blink, and changes from programs (DECSCUSR) | `printf '\e[6 q'` gives a beam                                       |  ✅   |  🧪   |   🧪    |                |
| REN-06 | Wide characters and emoji take two cells                                            | `echo '漢字 👍'`                                                     |  ✅   |  🧪   |   🧪    |                |
| REN-07 | Only changed rows repaint; a full-screen `cat` of a large file stays smooth         | `cat` a 50 MB log                                                    |  ✅   |  🧪   |   🧪    |                |

## Configuration and themes (CONF)

| ID      | Feature                                                                                      | How to check                                                       | Linux | macOS | Windows | Automated by               |
| ------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | :---: | :---: | :-----: | -------------------------- |
| CONF-01 | A commented `config.toml` is written on first run                                            | Remove it and start tersh                                          |  ✅   |  🧪   |   🧪    | cargo (tersh-adapters)     |
| CONF-02 | Everything hot-reloads: config, themes, `style.css`, plugins, installed fonts                | Edit each and save; the window updates without restarting          |  ✅   |  🧪   |   🧪    | cargo (tersh-app settings) |
| CONF-03 | Mistakes (unknown key, bad colour, unknown theme) show as a notification                     | Add `fnt = 1`; a toast names it                                    |  ✅   |  🧪   |   🧪    | cargo (tersh-domain)       |
| CONF-04 | 24 built-in themes with forgiving names; `[theme]` overrides; `themes/NAME.toml`             | `theme = "tokyonight"`, then a custom theme file                   |  ✅   |  🧪   |   🧪    | cargo (tersh-domain theme) |
| CONF-05 | `[env]` variables for the shell (they win over everything else)                              | `[env] FOO = "1"`; `echo $FOO` in a new window                     |  ✅   |  🧪   |   🧪    | cargo (session_with_fakes) |
| CONF-06 | Keybindings: defaults, overrides, `"none"`, `{ send = "…" }`                                 | Bind `ctrl+shift+g = { send = "git status\r" }`                    |  ✅   |  🧪   |   🧪    | vitest (input)             |
| CONF-07 | Actions: copy, paste, zoom, scroll, `clear_cards`, `reload_config`, `open_config`, `restart` | Use each default binding (Cmd on macOS, Ctrl/Ctrl+Shift elsewhere) |  ✅   |  🧪   |   🧪    |                            |
| CONF-08 | `open_config` opens the file in the system editor                                            | Ctrl/Cmd + `,`                                                     |  ✅   |  🧪   |   🧪    |                            |

## Rich cards (CARD)

| ID      | Feature                                                                                  | How to check                                            | Linux | macOS | Windows | Automated by               |
| ------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------- | :---: | :---: | :-----: | -------------------------- |
| CARD-01 | Cards from program output (`OSC 7701`), JSON or base64                                   | The shell example in docs/protocol.md                   |  ✅   |  🧪   |   🧪    | cargo (session_with_fakes) |
| CARD-02 | Cards over `$TERSH_SOCKET` with a reply per line (Unix)                                  | `tersh-show` with the socket set                        |  ✅   |  🧪   |    —    | cargo (tersh-show e2e)     |
| CARD-03 | `rich.osc = false` accepts cards only from the socket                                    | `cat` a file containing OSC 7701: no card               |  ✅   |  🧪   |   🧪    |                            |
| CARD-04 | Kinds: text, json, markdown + KaTeX, image, video, svg, html, plotly, notebook, output   | `tersh-show` one file of each kind                      |  ✅   |  🧪   |   🧪    | vitest (cards)             |
| CARD-05 | Video plays (WebM everywhere; MP4/H.264 where the OS has the codec)                      | `tersh-show clip.webm`                                  |  🧪   |  🧪   |   🧪    |                            |
| CARD-06 | Anchors: cursor (scrolls with output), cell (alt screen), float                          | One of each; scroll and open vim                        |  ✅   |  🧪   |   🧪    | vitest (cards)             |
| CARD-07 | Ops: show, update, move, close, clear, info                                              | `tersh-show --id x`, then `-u --id x`, then `--close x` |  ✅   |  🧪   |   🧪    | cargo (session_with_fakes) |
| CARD-08 | HTML cards are sandboxed: no terminal or IPC access, no network unless `html_network`    | An HTML card that calls `fetch` fails                   |  ✅   |  🧪   |   🧪    |                            |
| CARD-09 | Cards use the theme (colours, fonts, light/dark)                                         | Switch the theme with a notebook open                   |  ✅   |  🧪   |   🧪    |                            |
| CARD-10 | Alt-screen cards are removed when the program exits; cursor cards leave with the history | Quit vim with a card open                               |  ✅   |  🧪   |   🧪    | vitest (cards)             |

## tersh-show (SHOW)

| ID      | Feature                                                                   | How to check                                                               | Linux | macOS | Windows | Automated by             |
| ------- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------- | :---: | :---: | :-----: | ------------------------ |
| SHOW-01 | Kind from the file name or content; `-k` to force; `-` reads stdin        | `tersh-show a.png b.md c.ipynb`; `echo '# hi' \| tersh-show -k markdown -` |  ✅   |  🧪   |   🧪    | cargo (tersh-show)       |
| SHOW-02 | Inline cards reserve rows (`-r`); notebooks float by default; `-f` floats | Run each                                                                   |  ✅   |  🧪   |   🧪    | cargo (tersh-show e2e)   |
| SHOW-03 | `--pane` covers the current tmux pane                                     | In a split, `tersh-show --pane clip.webm`                                  |  ✅   |  🧪   |    —    | cargo (tersh-show)       |
| SHOW-04 | `--serve`: HTTP `POST /cards` with a bearer token, batches, CORS          | docs/building-on-tersh.md curl example; a wrong token gets 401             |  ✅   |  🧪   |   🧪    | cargo (tersh-show serve) |
| SHOW-05 | Works over ssh and in tmux via escape sequences (`--osc`)                 | `ssh host tersh-show img.png` inside tersh                                 |  ✅   |  🧪   |   🧪    | cargo (tersh-show e2e)   |

## tmux (TMUX)

| ID      | Feature                                                    | How to check                                                 | Linux | macOS | Windows | Automated by |
| ------- | ---------------------------------------------------------- | ------------------------------------------------------------ | :---: | :---: | :-----: | ------------ |
| TMUX-01 | Cards pass through tmux (`allow-passthrough on`)           | `tersh-show img.png` in tmux                                 |  ✅   |  🧪   |    —    |              |
| TMUX-02 | Panes use the socket via `update-environment TERSH_SOCKET` | New pane: `echo $TERSH_SOCKET`                               |  ✅   |  🧪   |    —    |              |
| TMUX-03 | Status bars and popups (e.g. floax) use the terminal theme | Status bar icons render; a floax popup's text uses the theme |  ✅   |  🧪   |    —    |              |
| TMUX-04 | Restart on update re-attaches to tmux                      | Update with tmux running                                     |  ✅   |  🧪   |    —    |              |

## Neovim plugin (NVIM)

| ID      | Feature                                                                                        | How to check                                    | Linux | macOS | Windows | Automated by |
| ------- | ---------------------------------------------------------------------------------------------- | ----------------------------------------------- | :---: | :---: | :-----: | ------------ |
| NVIM-01 | Installs with lazy.nvim from the repo root (`main = "tersh"`, `lazy = false`)                  | `:Lazy`; `:checkhealth tersh`                   |  ✅   |  🧪   |   🧪    | nvim         |
| NVIM-02 | `.ipynb` files open with a live rendered preview                                               | `nvim notebook.ipynb`                           |  ✅   |  🧪   |   🧪    | nvim         |
| NVIM-03 | `:TershPreview` for markdown, HTML, SVG, Plotly JSON; the last good render stays while editing | Edit a markdown file with the preview open      |  ✅   |  🧪   |   🧪    | nvim         |
| NVIM-04 | `:TershShow` (images, video, any kind) pinned below a line                                     | `:TershShow img.png`; scroll, edit, fold, split |  ✅   |  🧪   |   🧪    | nvim         |
| NVIM-05 | Cards follow lines through scrolling, edits, folds, splits, tabs, and tmux panes               | As above, inside tmux                           |  ✅   |  🧪   |    —    | nvim         |
| NVIM-06 | Cards hide while Neovim is suspended (Ctrl-Z)                                                  | Ctrl-Z, `fg`                                    |  ✅   |  🧪   |    —    | nvim         |
| NVIM-07 | Lua API: `attach`, `attach_window`, `update`, `close`, `send`, `available`                     | docs/neovim.md examples                         |  ✅   |  🧪   |   🧪    | nvim         |
| NVIM-08 | `:checkhealth tersh` reports the connection and the tmux settings                              | Run it inside and outside tersh                 |  ✅   |  🧪   |   🧪    |              |

## Plugins and building on tersh (EXT)

| ID     | Feature                                                                         | How to check                                            | Linux | macOS | Windows | Automated by           |
| ------ | ------------------------------------------------------------------------------- | ------------------------------------------------------- | :---: | :---: | :-----: | ---------------------- |
| EXT-01 | JavaScript plugins: renderers, actions, keybindings, events, styles; hot reload | `examples/plugins/csv.js` in `~/.config/tersh/plugins/` |  ✅   |  🧪   |   🧪    | vitest (plugins)       |
| EXT-02 | `[plugins] disabled` turns a plugin off without deleting it                     | Disable `csv`                                           |  ✅   |  🧪   |   🧪    | vitest (plugins)       |
| EXT-03 | `@tersh/cards` renders every kind in a plain web page                           | `npx vite packages/cards/example`                       |  ✅   |  🧪   |   🧪    | vitest (cards package) |
| EXT-04 | Demos: video in a tmux pane, a notebook in Neovim                               | `examples/demos/*.sh`                                   |  ✅   |  🧪   |    —    |                        |

## Known issues

- **INST-03, WIN-01 (macOS):** reported as not launching like a normal app from
  Spotlight. To be diagnosed on a Mac.
- **WIN-04 (macOS):** reported as not transparent, though transparency,
  `macOSPrivateApi` and vibrancy are all set. To be diagnosed on a Mac.
- **WIN-08 (macOS):** apps started from Spotlight get launchd's minimal `PATH`.
  tersh doesn't start a login shell yet, so Homebrew and profile settings may
  be missing.
- **WIN-03, INST-09, INST-10 (Windows):** the app is built as a GUI program, so
  `tersh --version` and `tersh update` may print nothing in a console.
  `tersh update` also has to replace a running `tersh.exe`. Both are
  unverified.
- **CARD-02 (Windows):** the rich socket is Unix-only. Windows programs use
  escape sequences, or `tersh-show --serve` over HTTP.
