# Configuration

Sunset Commander reads a single optional JSON file at `~/.config/sc/config.json`. The app
runs with built-in defaults if the file is absent, and any key you omit falls back to its
default individually — you only need to specify the settings you want to change.

If the file doesn't exist yet and sc can locate its bundled helper scripts (see
[Bundled scripts](#bundled-scripts) below), it generates a starter `config.json` with a
`View`/`Edit`/`Edit config` menu wired up to those scripts.

The file has five top-level sections, all optional:

```jsonc
{
  "keybindings": { /* ... */ },
  "startup":     { /* ... */ },
  "menu":        [ /* ... */ ],
  "colorscheme": { /* ... */ },
  "panels":      { /* ... */ }
}
```

## Keybindings

Each entry maps an action name to one key spec, or an array of key specs (any of them
triggers the action):

```jsonc
{
  "keybindings": {
    "copy": "F5",
    "exit": ["F10", "Ctrl-q"]
  }
}
```

### Key spec syntax

- Modifier prefixes, combinable and in any order: `C-`/`Ctrl-` (Control), `A-`/`Alt-` (Alt),
  `S-`/`Shift-` (Shift). Example: `Ctrl-Alt-b`.
- Named keys: `Enter`, `Tab`, `Backspace`, `Delete`/`Del`, `Insert`, `Up`, `Down`, `Left`,
  `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Esc`, `F1`–`F12`.
- A single printable character otherwise, e.g. `,`, `/`, `b`.
- A **chord** (two keys pressed in sequence, à la Emacs) is written as two key specs
  separated by one space, e.g. `"Ctrl-x t"` for Ctrl-x then t.

### Available actions and their defaults

| Action | Default |
|---|---|
| `go_to_parent` | `Alt-Up` |
| `go_back` | `Alt-Left` |
| `go_forward` | `Alt-Right` |
| `switch_panel` | `Tab` |
| `toggle_layout` | `Alt-,` |
| `sync_panels` | `Alt-i` |
| `refresh_panel` | `Alt-r` |
| `sort_panel` | `Ctrl-s` |
| `quicksearch` | `/`, `Alt-s` |
| `filter` | `Alt-f` |
| `search` | `Alt-?`, `Ctrl-f` |
| `toggle_hidden` | `Alt-.` |
| `bookmark_open` | `Ctrl-\` |
| `bookmark_add` | `Ctrl-b` |
| `tag_file` | `Insert` |
| `invert_tags` | `*` |
| `select_group` | `+` |
| `unselect_group` | `-` |
| `copy` | `F5` |
| `move` | `F6` |
| `delete` | `F8` |
| `rename` | `Shift-F6` |
| `mkdir` | `F7` |
| `cmdline_insert_filename` | `Alt-Enter`, `Ctrl-Enter` |
| `cmdline_insert_fullpath` | `Ctrl-Shift-Enter` |
| `cmdline_complete` | `Alt-Tab`, `Ctrl-Space` |
| `cmdline_insert_tagged` | `Ctrl-x t` |
| `cmdline_insert_tagged_other` | `Ctrl-x Ctrl-t` |
| `cmdline_insert_path` | `Ctrl-x p` |
| `cmdline_insert_path_other` | `Ctrl-x Ctrl-p` |
| `cmdline_history_prev` | `Ctrl-Up` |
| `cmdline_history_next` | `Ctrl-Down` |
| `reverse_search` | `Ctrl-r`, `Alt-h` |
| `toggle_shell` | `Ctrl-o` |
| `toggle_shell_and_sync_command_line` | `Alt-o` |
| `toggle_cmdline` | `Ctrl-Alt-b` |
| `toggle_button_bar` | `Alt-b` |
| `user_menu` | `F2` |
| `exit` | `F10`, `Ctrl-q` |
| `path_history` | `Alt-H`, `Alt-Down` |
| `toggle_matches_panel` | `Alt-m` |
| `view` | `F3` |

Notes:

- Unrecognized action names in `keybindings` are ignored.
- A handful of interaction keys (Up/Down/PageUp/PageDown/Home/End/Enter/Esc for list and
  popup navigation, and Del to remove an entry from the bookmarks popup) are fixed and not
  configurable — see the "non-configurable" note in
  [`CheatSheet.md`](CheatSheet.md).

## Startup

```jsonc
{
  "startup": {
    "restore_paths": false,
    "subshell": true,
    "ipc_scripting": false
  }
}
```

- `restore_paths` — if `true`, panels restore the last-visited paths from
  `~/.local/state/sc/state.json` on launch instead of starting at the current working
  directory. Defaults to `false`.
- `subshell` — if `true`, `Ctrl-o` toggles a full interactive subshell instead of a stateless
  output overlay. Defaults to `true`.
- `ipc_scripting` — if `true`, IPC actions beyond `ShowPanels` are enabled (see
  [IpcActions.md](IpcActions.md)). Defaults to `false`.

All three can be overridden per-launch on the command line; see
[`CommandLineArgs.md`](CommandLineArgs.md).

## User menu

`menu` is a list of commands shown when `user_menu` (`F2`) is pressed:

```jsonc
{
  "menu": [
    { "label": "View", "command": "less %f", "keys": "Shift-F3" },
    { "label": "Edit", "command": "$EDITOR %f", "keys": "F4" },
    { "label": "Diff", "command": "diff %f %F" },
    { "label": "Word count", "command": "wc -l %t", "add_to_bar": true }
  ]
}
```

- `label` — text shown in the user menu.
- `command` — shell command to run; may use the macros described in
  [`MacroSubstitution.md`](MacroSubstitution.md) (`%f`, `%F`, `%d`, `%D`, `%t`, `%T`,
  `%u`, `%U`, `%s`, `%S`, `%b`, `%%`) to reference the selected/tagged files and panel
  directories.
- `keys` (optional) — a key spec (same syntax as keybindings) that runs the command
  directly, without opening the menu.
- `add_to_bar` (optional, default `false`) — also show this entry as a clickable button in
  the button bar.

## Bundled scripts

sc ships two helper scripts, `view.sh` and `edit.sh`, used by the auto-generated default
menu. They're resolved at runtime by checking, in order:

1. `<install prefix>/share/sc/scripts/` (the install prefix is fixed at compile time,
   `/usr/local` by default).
2. A `scripts/` directory next to the running binary (used for `cargo run` / development
   builds).

`edit.sh` uses `$EDITOR`, falling back to common editors if unset; `view.sh` uses `$PAGER`,
falling back to `less`/`more`.

## Color scheme

All colors are `#rrggbb` hex strings:

```jsonc
{
  "colorscheme": {
    "panel_bg":            "#1a1a2e",
    "panel_fg":            "#eaeaea",
    "active_border_fg":    "#00aaff",
    "inactive_border_fg":  "#555555",
    "selected_fg":         "#ffff00",
    "selected_bg":         "#1a3a5c",
    "tagged_fg":           "#ff8c00",
    "tagged_bg":           "#2d1500",
    "cmdline_bg":          "#000000",
    "cmdline_fg":          "#ffffff",
    "cmdline_inactive_bg": "#000000",
    "cmdline_inactive_fg": "#888888",
    "dialog_bg":           "#003366",
    "dialog_fg":           "#ffffff",
    "dialog_border_fg":    "#00aaff",
    "dialog_butt_fg":      "#000000",
    "dialog_butt_bg":      "#00aaff",
    "dialog_error_fg":     "#ff4444",
    "dialog_mark_fg":      "#00ff88",
    "panel_error_fg":      "#ff4444",
    "button_bar_bg":       "#000000",
    "button_bar_fg":       "#ffffff",
    "button_bar_butt_bg":  "#00aaff",
    "button_bar_butt_fg":  "#000000",
    "status_info_fg":      "#000000",
    "status_info_bg":      "#00aa55",
    "status_warn_fg":      "#000000",
    "status_warn_bg":      "#ddaa00",
    "search_match_fg":     "#000000",
    "search_match_bg":     "#ffcc00"
  }
}
```

The values above are the built-in defaults; only include the keys you want to change.

## Panels

```jsonc
{
  "panels": {
    "time_format": "%y-%m-%d %H:%M",
    "time_lenght": 14,
    "default_action_executable": "",
    "default_action_text": ":view",
    "default_action": ""
  }
}
```

- `time_format` — [`chrono`](https://docs.rs/chrono) `strftime` format string used for the
  Mtime column in the panel listing. Defaults to `%y-%m-%d %H:%M`.
- `time_lenght` — width in characters of the Mtime column (header and cells). Formatted
  dates are truncated or padded to this width, so it should match the length produced by
  `time_format`. Defaults to `14`.
- `default_action_executable` / `default_action_text` / `default_action` — what Enter and
  double-click do on a file (never on a directory) in the active panel. `default_action_executable`
  applies when the file has any executable permission bit set; `default_action_text`
  applies otherwise. Either falls back to `default_action` when left empty (`""`), and an
  empty `default_action` is a no-op — sc does nothing, same as today's behavior, unless
  you opt in. `default_action_text` defaults to `:view`.

  A value of `:view` is the one built-in command currently supported: it opens the
  internal text viewer, the same as pressing `F3` (action `view`). Any other
  non-empty value is a shell command template, run the same way as a
  [user menu](#user-menu) command, except only the current-file macros — `%f`, `%x`,
  `%b`, `%d` — are meaningful; the inactive-panel and tagged-files macros (`%F`, `%D`,
  `%t`, `%T`, `%u`, `%U`, `%s`, `%S`) don't apply to a single-entry default action and
  expand to empty. See @MacroSubstitution.md.

## Related files

Besides `config.json`, sc keeps a few other files under `~/.config/sc/` and
`~/.local/state/sc/`:

| File | Contents |
|---|---|
| `~/.config/sc/config.json` | This file. |
| `~/.config/sc/bookmarks.json` | Bookmarked directories (`Ctrl-b` to add, `Ctrl-\` to browse). |
| `~/.local/state/sc/state.json` | Panel layout, visibility toggles, and last-visited paths. |
| `~/.local/state/sc/command_history` | Command line history. |
| `~/.local/state/sc/panel_history.json` | Per-panel directory navigation history. |

None of these need to be created or edited by hand under normal use.
