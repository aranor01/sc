# sc — Command Line Arguments

## Synopsis

```
sc [OPTIONS] [DIR1] [DIR2]
```

## Arguments

| Argument | Description |
|---|---|
| `dir1` | Starting path for both panels. |
| `dir2` | Starting path for the right panel only. Requires `dir1`. |

## Options

| Option | Description |
|---|---|
| `--restore-paths` | Override `startup.restore_paths` in config: restore panel paths from `~/.local/state/sc/state.json`. Conflicts with `--no-restore-paths`. |
| `--no-restore-paths` | Override `startup.restore_paths` in config: start panels at the current working directory. Conflicts with `--restore-paths`. |
| `--subshell` | Override `startup.subshell` in config: start in subshell mode. Conflicts with `--no-subshell`. |
| `--no-subshell` | Override `startup.subshell` in config: start in stateless mode. Conflicts with `--subshell`. |
| `--ipc-scripting` | Override `startup.ipc_scripting` in config: enable IPC actions beyond `ShowPanels` (see [IpcActions.md](IpcActions.md)). Conflicts with `--no-ipc-scripting`. |
| `--no-ipc-scripting` | Override `startup.ipc_scripting` in config: keep only `ShowPanels` enabled. Conflicts with `--ipc-scripting`. |
| `-d`, `--no-mouse` | Disable mouse support (no `EnableMouseCapture`), so the terminal's native mouse selection/copy-paste works instead. |
| `-h`, `--help` | Print help and exit. |
| `-V`, `--version` | Print version and exit. |

Passing an unrecognized option, an unexpected extra positional argument, or both flags of a
conflicting pair (e.g. `--restore-paths --no-restore-paths`) prints an error to stderr and
exits with a non-zero status.

## Panel path resolution

Each panel's starting path is determined in this priority order:

1. **Explicit directory argument(s)** — if any dir is given, both panels are set from the
   dirs and the flags are ignored entirely.
2. **`--restore-paths` / `--no-restore-paths` flag** — overrides the config setting (only
   consulted when no directory arguments are given).
3. **`startup.restore_paths` from config** (default `false` → current working directory).

### Interaction rules

- **Both `dir1` and `dir2` given**: left panel = `dir1`, right panel = `dir2`.
  `--restore-paths` / `--no-restore-paths` are silently ignored.

- **Only `dir1` given**: both panels start at `dir1`.
  `--restore-paths` / `--no-restore-paths` are silently ignored.

- **No directories given**: `--restore-paths` / `--no-restore-paths` controls both panels.

## Examples

| Invocation | Left panel | Right panel | Note |
|---|---|---|---|
| `sc` | cwd (or restored, per config) | cwd (or restored, per config) | |
| `sc --restore-paths` | restored from state | restored from state | overrides config |
| `sc --no-restore-paths` | cwd | cwd | overrides config |
| `sc /tmp` | `/tmp` | `/tmp` | flag ignored |
| `sc /tmp --restore-paths` | `/tmp` | `/tmp` | flag ignored |
| `sc /tmp --no-restore-paths` | `/tmp` | `/tmp` | flag ignored |
| `sc /tmp /var` | `/tmp` | `/var` | flag ignored |
| `sc /tmp /var --restore-paths` | `/tmp` | `/var` | flag ignored |
| `sc /tmp /var --no-restore-paths` | `/tmp` | `/var` | flag ignored |
