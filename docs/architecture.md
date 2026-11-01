# Architecture

tersh uses a hexagonal (ports and adapters) layout. The terminal's rules live
in the middle and don't know about any operating system, emulator library or
UI toolkit. Everything outside connects through interfaces called ports.

```text
              driving adapters                         driven adapters
        ┌──────────────────────────┐            ┌──────────────────────────────┐
        │ src-tauri (desktop app)  │            │ tersh-adapters               │
        │ tersh-show (CLI)         │            │   pty        portable-pty    │
        │ editors/nvim (plugin)    │            │   emulator   alacritty       │
        └────────────┬─────────────┘            │   socket     Unix socket     │
                     │ use cases                │   config_fs  ~/.config/tersh │
             ┌───────▼──────────────────────────┴───────▲──────────────────┐
             │ tersh-app       session · settings · ports (traits)          │
             │   ┌─────────────────────────────────────────────────────┐   │
             │   │ tersh-domain   config · themes · rich protocol ·    │   │
             │   │                frames · palette   (no I/O)          │   │
             │   └─────────────────────────────────────────────────────┘   │
             └──────────────────────────────────────────────────────────────┘
```

## Rust crates

| Crate            | Layer           | Contents                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ---------------- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tersh-domain`   | domain          | Config model and rules, theme resolution (24 built-in themes), the OSC 7701 scanner and anchor rules, frame and palette types, version comparison and the restart-when-safe rule, and `AppError` with its registered codes. Depends only on serde/serde_json/base64; file formats are decoded by adapters. User themes arrive already decoded through the `ThemeSource` trait.                                                                                                                                                                                                                                                              |
| `tersh-app`      | application     | Ports (`PtySystem`, `EmulatorFactory`, `TermHost`, `ConfigStore`, `Sink`) and the use cases: `Session` (moves bytes between PTY, emulator and UI, dispatches rich messages, exposes a `RichInbox`; for a `Sink` that acknowledges frames it keeps one in flight, so a UI is never sent frames faster than it draws them) and `Settings` (snapshot and change detection), and `update` (check and install releases). Further ports: `ProcessInspector`, `ReleaseSource`, `Installer`, `Watchable`, and `TimePort`, the use cases' only clock. `instrumented::Instrumented` is a decorator that times any `EmulatorFactory` for the profiler. |
| `tersh-adapters` | driven adapters | `NativePty` (portable-pty), `AlacrittyEmulators` (frame building, scroll tracking), `socket` (newline-JSON server on `$TERSH_SOCKET`), `FsConfigStore` (config, themes, style.css, plugins), `SystemFontDirs`, `SystemProcesses` (foreground program, cwd), `GithubReleases` and `InstallScript` (updates), `AppBinary` (watching and relaunching the executable), `SystemClock`, `toml_format` (decoding config and theme files). `spawn_local` wires the native ones together, with the emulator wrapped in `Instrumented`.                                                                                                               |
| `src-tauri`      | driving adapter | The desktop window and composition root: Tauri commands and channels (the page acknowledges each painted frame with `session_frame_shown`), live config reload, the sandboxed `tersh-card:` protocol for HTML cards. `environment.rs` reads the process environment once and passes it to the adapters.                                                                                                                                                                                                                                                                                                                                     |
| `tersh-show`     | driving adapter | A CLI that speaks the rich protocol to a running terminal, and `--serve`, an HTTP bridge to it. Standalone: it depends on no tersh crate, only on the protocol.                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

Dependencies only point inward: `tersh-domain` ← `tersh-app` ← `tersh-adapters`
← `src-tauri`. Cargo enforces this, because a crate can't use one it doesn't
depend on.

Rules inside the layers, checked by `crates/tersh-adapters/tests/architecture.rs`:

- **The domain does no I/O,** not even logging: the OSC scanner reports a
  dropped oversized payload as `Chunk::Oversized` and the session logs it.
- **Only entry points read the environment.** `src-tauri/src/environment.rs`
  reads it once; adapters get what they need as arguments (`AppBinary::locate`,
  `SystemFontDirs::new`, `InstallScript::new`, `unbundled`).
- **Time comes through `TimePort`.** The session paces frames on it. How long
  parsing and frame building take is measured outside the session, by the
  `Instrumented` decorator the entry point wraps around the emulator.
- **Expected failures are `AppError`s** with a code from `docs/errors.md`.
  Adapters translate library errors at the boundary and keep them as the
  cause; bugs panic. Commands and socket replies carry the rendered text,
  `message: cause (CODE)`.

## Frontend (`src/`)

The page is a hexagon of its own, with the same layers:

| Directory        | Layer           | Contents                                                                                                                                                                                                                 |
| ---------------- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `domain/`        | domain          | Shared types, key and mouse encoding, palette resolution, keybinding tables, theme-to-CSS-variable mapping, card anchor geometry, selection geometry, wheel handling, profiler maths, the plugin event types.            |
| `ports/`         | ports           | `Backend` (the host: session I/O, settings, clipboard, window, card documents) and `Screen` (what the terminal draws on).                                                                                                |
| `app/`           | use case        | `TerminalApp`: what keys, mouse reports, actions and session events mean; the action and keybinding registries; zoom; telling the host the window is ready. Talks only to `Backend` and `Screen`; tested with fakes.     |
| `adapters/dom`   | driving adapter | The page: `DomScreen` (canvas renderer, selection, cards, toasts) implements `Screen` and turns DOM events into calls on `TerminalApp`. Also the profiler overlay, the plugin API and loader, fonts and the style sheet. |
| `adapters/tauri` | driven adapter  | `TauriBackend`, the only code that imports `@tauri-apps/*`.                                                                                                                                                              |
| `main.ts`        | composition     | Creates `TauriBackend` and `DomScreen`, wraps the backend in the profiler (`profiled`), and wires them to `TerminalApp`.                                                                                                 |

The card renderers live in [`packages/cards`](../packages/cards) (`@tersh/cards`).
It is a standalone package with no tersh dependencies, so web pages can use it
too. The app builds it from source through a Vite alias and plugs in its own
`tersh-card:` document host.

`src/__tests__/architecture.test.ts` fails the build if a layer imports one it
shouldn't (for example `app/` importing an adapter, or the page importing
Tauri), or if `domain/`, `ports/` or `app/` touch the DOM.

## Testing at each layer

- **Domain:** plain unit tests (`cargo test -p tersh-domain`, and the
  `domain/` tests in vitest).
- **Page use case:** `src/__tests__/app.test.ts` drives `TerminalApp` with a
  fake backend and screen.
- **Application:** `crates/tersh-app/tests/session_with_fakes.rs` drives a
  `Session` with an in-memory PTY and emulator. No processes or terminal
  library are involved.
- **Adapters:** `crates/tersh-adapters/tests` run real shells, the real
  socket, and Neovim with the plugin inside a PTY.
- **End to end:** `crates/tersh-show/tests/e2e.rs` runs the CLI inside a real
  session.

## Adding things

| To add                                 | Do this                                                                                 |
| -------------------------------------- | --------------------------------------------------------------------------------------- |
| a built-in theme                       | add an entry to `BUILTINS` in `crates/tersh-domain/src/theme.rs`; the tests validate it |
| a config option                        | add the field in `tersh-domain/src/config.rs` and document it in `config.template.toml` |
| another emulator or PTY implementation | implement `EmulatorFactory` / `PtySystem` from `tersh-app::ports`                       |
| another host (web, other toolkit)      | implement `Backend` from `src/ports/backend.ts` and compose it in a new entry point     |
| a card kind or action                  | write a plugin (see [plugins.md](plugins.md)); no core change needed                    |
