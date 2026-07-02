# sc — Scripting / IPC Actions

While sc is running, it listens for actions on a local Unix domain socket and exports the
socket's path as `$SC_TOKEN` in its own environment, so any command launched from the
command line or the user menu inherits it automatically.

## Security

Only processes running as the same Unix user as `sc` can talk to the socket — this is
checked with `SO_PEERCRED` on every connection, not just inferred from file permissions,
so it still holds if the socket ends up somewhere more exposed than usual (e.g. under
`sudo sc`, where `$XDG_RUNTIME_DIR` is typically unset and the socket falls back to a
shared temp directory). The socket file is also always created mode `0600`, regardless of
the umask in effect when `sc` started. A connection that doesn't finish sending a message
within 500ms, or that sends more than 1 MiB, is dropped.

There's no scoping *within* a valid connection: `$SC_TOKEN` is all-or-nothing access to
every action in the table below, and every child process launched from the command line or
user menu gets it automatically. Treat it like any other credential your child processes —
and whatever they in turn run — can see.

## `sc-action`

A companion binary, installed alongside `sc`, sends one action per invocation:

```
sc-action <token> <action> [args...]
sc-action <token> <action> -       # read args from stdin, one per line
```

If `SC_TOKEN` is unset or invalid, `sc-action` exits quietly without doing anything.

## Actions

| Action | Args | Effect |
|---|---|---|
| `Tag` | filenames | Tag the given entries in the active panel (additive). Names not present in the panel's current listing are ignored. |
| `Untag` | filenames | Untag the given entries in the active panel. |
| `TagOnly` | filenames | Clear all tags in the active panel, then tag the given entries. |
| `SelectGroup` | pattern | Tag entries in the active panel matching the pattern. |
| `UnselectGroup` | pattern | Untag entries in the active panel matching the pattern. |
| `Filter` | pattern | Set the active panel's filter to the pattern; an empty pattern clears it. |
| `InjectToCommandLine` | text | Insert text into the command line, showing it if hidden. An optional mode may precede the text: `Insert` (default) at the current cursor position, `Append` at the end, or `Replace` for the entire command line. |
| `ToggleShell` | — | Toggle the output/shell overlay, same as `Ctrl-o`. |
| `RefreshPanel` | — | Force the active panel to re-read its directory from disk. |
| `ShowPanels` | optional directory | Return from the output/shell overlay to the panel view; if a directory is given and differs from the active panel's current path, navigate there. |

Filenames given to `Tag` / `Untag` / `TagOnly` may be full paths — only the last path
component (the basename) is matched against the active panel's listing.

Patterns given to `SelectGroup` / `UnselectGroup` / `Filter` are shell globs (e.g. `*.rs`)
by default; prefix with `/` to use a regular expression instead (e.g. `/\.rs$` matches any
name ending in `.rs`). Matching is case-sensitive and applies to both files and directories.

For `InjectToCommandLine`, the optional mode must be exactly `Insert`, `Append`, or
`Replace` as the first argument; if the first argument doesn't match one of these,
it's treated as part of the text and `Insert` is used.

## Examples

```sh
# From a script or user-menu command: tag every file a build just touched.
git diff --name-only | sc-action "$SC_TOKEN" TagOnly -
```

```sh
# Insert at the cursor (default), or explicitly append/replace.
sc-action "$SC_TOKEN" InjectToCommandLine "picked-file.txt"
sc-action "$SC_TOKEN" InjectToCommandLine Append "picked-file.txt"
```
