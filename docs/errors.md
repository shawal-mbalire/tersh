# Error codes

Every expected failure in tersh is an `AppError` with one of these codes
(`crates/tersh-domain/src/error.rs`). Messages end with the code, for example
`could not start zsh: No such file or directory (PTY-002)`, so a log line or
an error in the window can be looked up here.

- **validation:** the input was invalid; fix the input.
- **external:** something outside tersh failed (the OS, a program, the network).
- **contract:** something outside tersh answered in a way it shouldn't have; report it.

A test fails if a code in the source is missing from this table.

| Code      | Family     | What to do                                               |
| --------- | ---------- | -------------------------------------------------------- |
| `CFG-001` | validation | Fix the TOML at the reported line in config.toml.        |
| `CFG-002` | external   | Check that config.toml is readable.                      |
| `CFG-003` | external   | Check that the config directory is writable.             |
| `THM-001` | validation | Fix the TOML in the theme file.                          |
| `THM-002` | external   | Check that the theme file is readable.                   |
| `RCH-001` | validation | Send a JSON object, raw or base64-encoded.               |
| `SOC-001` | external   | Check that the runtime directory is writable.            |
| `PTY-001` | external   | The system may be out of pseudo-terminals.               |
| `PTY-002` | external   | Check `shell` in config.toml, or $SHELL.                 |
| `PTY-003` | external   | The terminal may have closed.                            |
| `SES-001` | external   | The program has exited; restart the terminal.            |
| `SES-002` | external   | The system may be out of threads or memory.              |
| `SES-003` | validation | Start a session before sending it input.                 |
| `UPD-001` | external   | Check the network connection and that curl is installed. |
| `UPD-002` | contract   | GitHub's reply changed; report it as a tersh bug.        |
| `UPD-003` | external   | Run the install command by hand to see its output.       |
| `BIN-001` | external   | Reinstall tersh.                                         |
| `BIN-002` | external   | Check that the file can be run.                          |
| `APP-001` | external   | Check that a program is set to open the file type.       |
| `CLI-001` | validation | Run `tersh --help` for usage.                            |
