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

| Crate            | Layer           | Contents                                                                                                                                                                                                                                                                                                                                                                                                         |
| ---------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tersh-domain`   | domain          | Config model and parsing, theme resolution (24 built-in themes), the OSC 7701 scanner and anchor rules, frame and palette types, version comparison and the restart-when-safe rule. Depends only on serde/toml/base64. User theme files arrive through the `ThemeSource` trait.                                                                                                                                  |
| `tersh-app`      | application     | Ports (`PtySystem`, `EmulatorFactory`, `TermHost`, `ConfigStore`, `Sink`) and the use cases: `Session` (moves bytes between PTY, emulator and UI, dispatches rich messages, exposes a `RichInbox`) and `Settings` (snapshot and change detection), and `update` (check and install releases). Further ports: `ProcessInspector`, `ReleaseSource`, `Installer`, `Watchable`.                                      |
| `tersh-adapters` | driven adapters | `NativePty` (portable-pty), `AlacrittyEmulators` (frame building, scroll tracking), `socket` (newline-JSON server on `$TERSH_SOCKET`), `FsConfigStore` (config, themes, style.css, plugins), `SystemFontDirs`, `SystemProcesses` (foreground program, cwd), `GithubReleases` and `InstallScript` (updates), `AppBinary` (watching and relaunching the executable). `spawn_local` wires the native ones together. |
| `src-tauri`      | driving adapter | The desktop window and composition root: Tauri commands and channels, live config reload, the sandboxed `tersh-card:` protocol for HTML cards.                                                                                                                                                                                                                                                                   |
| `tersh-show`     | driving adapter | A CLI that speaks the rich protocol to a running terminal.                                                                                                                                                                                                                                                                                                                                                       |

Dependencies only point inward: `tersh-domain` ← `tersh-app` ← `tersh-adapters`
← `src-tauri`. Cargo enforces this, because a crate can't use one it doesn't
depend on.

## Frontend (`src/`)

| Directory        | Layer       | Contents                                                                                                                          |
| ---------------- | ----------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `domain/`        | domain      | Shared types, key and mouse encoding, palette resolution, keybinding tables, theme-to-CSS-variable mapping, card anchor geometry. |
| `ports/`         | port        | `Backend`: session I/O, settings, clipboard, window, card documents.                                                              |
| `adapters/tauri` | adapter     | `TauriBackend`, the only code that imports `@tauri-apps/*`.                                                                       |
| `ui/`            | UI          | Canvas grid renderer, terminal controller, card manager and renderers, plugin API and loader, style sheet.                        |
| `main.ts`        | composition | Creates `TauriBackend` and hands it to the UI.                                                                                    |

`src/__tests__/architecture.test.ts` fails the build if a layer imports one it
shouldn't. For example, it fails if `ui/` imports Tauri or `domain/` imports
`ui/`.

## Testing at each layer

- **Domain:** plain unit tests (`cargo test -p tersh-domain`, and the
  `domain/` tests in vitest).
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
