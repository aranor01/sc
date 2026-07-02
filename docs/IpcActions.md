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
within 500ms, or that sends more than 8 MiB, is dropped.

By default, only `ShowPanels` is enabled; every other action below requires opting in via
`ipc_scripting` in config.json (or `--ipc-scripting` on the command line — see
[CommandLineArgs.md](CommandLineArgs.md)).

There's no scoping *within* that opt-in: `$SC_TOKEN` is then all-or-nothing access to every
action, and every child process launched from the command line or user menu gets it
automatically — treat it like any other secret visible to those processes and everything
they launch in turn. Misuse ranges from mild (a same-uid peer can keep reconnecting to
degrade UI responsiveness — no single stall is unbounded, but nothing caps how often it
repeats) to serious: `InjectToCommandLine` can queue a command into your command line for
you to run unknowingly, and `Tag`/`TagOnly`/`SelectGroup` can retarget what F5/F6/F8 act on.

Text handed to `InjectToCommandLine` is filtered before it reaches the command line: control
characters (so a connected peer can't smuggle terminal escape sequences into what gets
rendered) and Unicode bidi-override characters (so it can't make the command line *display*
something different from what it actually contains) are stripped. Everything else, including
non-ASCII text, passes through unchanged. The same filter applies to every other way text
reaches the command line — typing, autocomplete, yanking, and copying a file or path name in
— since a file name can also carry arbitrary bytes.

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

Both examples below use actions other than `ShowPanels`, so they require `ipc_scripting`
to be enabled (see Security above) — otherwise `sc-action` runs normally but sc silently
ignores the message.

```sh
# From a script or user-menu command: tag every file a build just touched.
git diff --name-only | sc-action "$SC_TOKEN" TagOnly -
```

```sh
# Insert at the cursor (default), or explicitly append/replace.
sc-action "$SC_TOKEN" InjectToCommandLine "picked-file.txt"
sc-action "$SC_TOKEN" InjectToCommandLine Append "picked-file.txt"
```
