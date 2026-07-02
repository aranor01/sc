# sc — Scripting / IPC Actions

While sc is running, it listens for actions on a local Unix domain socket and exports the
socket's path as `$SC_TOKEN` in its own environment, so any command launched from the
command line or the user menu inherits it automatically.

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
| `InjectToCommandLine` | text | Insert text into the command line at the current cursor position, showing the command line if it's hidden. |
| `ToggleShell` | — | Toggle the output/shell overlay, same as `Ctrl-o`. |
| `RefreshPanel` | — | Force the active panel to re-read its directory from disk. |
| `ShowPanels` | optional directory | Return from the output/shell overlay to the panel view; if a directory is given and differs from the active panel's current path, navigate there. |

Filenames given to `Tag` / `Untag` / `TagOnly` may be full paths — only the last path
component (the basename) is matched against the active panel's listing.

Patterns given to `SelectGroup` / `UnselectGroup` / `Filter` are shell globs (e.g. `*.rs`)
by default; prefix with `/` to use a regular expression instead (e.g. `/\.rs$` matches any
name ending in `.rs`). Matching is case-sensitive and applies to both files and directories.

## Examples

```sh
# From a script or user-menu command: tag every file a build just touched.
git diff --name-only | sc-action "$SC_TOKEN" TagOnly -
```
