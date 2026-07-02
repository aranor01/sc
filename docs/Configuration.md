# Configuration

Sunset Commander reads a single optional JSON file at `~/.config/sc/config.json`. The app
runs with built-in defaults if the file is absent, and any key you omit falls back to its
default individually — you only need to specify the settings you want to change.

If the file doesn't exist yet and sc can locate its bundled helper scripts (see
[Bundled scripts](#bundled-scripts) below), it generates a starter `config.json` with a
`View`/`Edit`/`Edit config` menu wired up to those scripts.

The file has four top-level sections, all optional:

```jsonc
{
  "keybindings": { /* ... */ },
  "startup":     { /* ... */ },
  "menu":        [ /* ... */ ],
  "colorscheme": { /* ... */ }
}
```

## Keybindings

Each entry maps an action name to one key spec, or an array of key specs (any of them
triggers the action):

```jsonc
{
  "keybindings": {
    "copy": "F5",
    "exit": ["F10", "C-q"]
  }
}
```

### Key spec syntax

- Modifier prefixes, combinable and in any order: `C-`/`Ctrl-` (Control), `A-`/`Alt-` (Alt),
  `S-`/`Shift-` (Shift). Example: `C-A-b` or `Ctrl-Alt-b`.
- Named keys: `Enter`, `Tab`, `Backspace`, `Delete`/`Del`, `Insert`, `Up`, `Down`, `Left`,
  `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Esc`, `F1`–`F12`.
- A single printable character otherwise, e.g. `,`, `/`, `b`.
- A **chord** (two keys pressed in sequence, à la Emacs) is written as two key specs
  separated by one space, e.g. `"C-x t"` for Ctrl-x then t.

### Available actions and their defaults

| Action | Default |
|---|---|
| `go_to_parent` | `A-Up` |
| `go_back` | `A-Left` |
| `go_forward` | `A-Right` |
| `switch_panel` | `Tab` |
| `toggle_layout` | `A-,` |
| `sync_panels` | `A-i` |
| `refresh_panel` | `A-r` |
| `sort_panel` | `C-s` |
| `quicksearch` | `/`, `A-s` |
| `filter` | `C-f` |
| `toggle_hidden` | `A-.` |
| `bookmark_open` | `C-\` |
| `bookmark_add` | `C-b` |
| `tag_file` | `Insert` |
| `invert_tags` | `*` |
| `select_group` | `+` |
| `unselect_group` | `-` |
| `copy` | `F5` |
| `move` | `F6` |
| `delete` | `F8` |
| `rename` | `S-F6` |
| `mkdir` | `F7` |
| `cmdline_insert_filename` | `A-Enter`, `C-Enter` |
| `cmdline_insert_fullpath` | `C-S-Enter` |
| `cmdline_complete` | `A-Tab`, `C-Space` |
| `cmdline_insert_tagged` | `C-x t` |
| `cmdline_insert_tagged_other` | `C-x C-t` |
| `cmdline_insert_path` | `C-x p` |
| `cmdline_insert_path_other` | `C-x C-p` |
| `cmdline_history_prev` | `C-Up` |
| `cmdline_history_next` | `C-Down` |
| `reverse_search` | `C-r`, `A-h` |
| `toggle_shell` | `C-o` |
| `toggle_shell_and_sync_command_line` | `A-o` |
| `toggle_cmdline` | `C-A-b` |
| `toggle_button_bar` | `A-b` |
| `user_menu` | `F2` |
| `exit` | `F10`, `C-q` |
| `path_history` | `A-H`, `A-Down` |

Notes:

- The JSON key for the Move action is `move`, not `move_entry`.
- Unrecognized action names in `keybindings` are ignored rather than rejected, so old or
  misspelled entries don't prevent the rest of the file from loading.
- A handful of interaction keys (Up/Down/PageUp/PageDown/Home/End/Enter/Esc for list and
  popup navigation, and Del to remove an entry from the bookmarks popup) are fixed and not
  configurable — see the "non-configurable" note in
  [`CheatSheet.md`](CheatSheet.md).

## Startup

```jsonc
{
  "startup": {
    "restore_paths": false,
    "subshell": true
  }
}
```

- `restore_paths` — if `true`, panels restore the last-visited paths from
  `~/.local/share/sc/state.json` on launch instead of starting at the current working
  directory. Defaults to `false`.
- `subshell` — if `true`, `C-o` toggles a full interactive subshell instead of a stateless
  output overlay. Defaults to `true`.

Both can be overridden per-launch on the command line; see
[`CommandLineArgs.md`](CommandLineArgs.md).

## User menu

`menu` is a list of commands shown when `user_menu` (`F2`) is pressed:

```jsonc
{
  "menu": [
    { "label": "View", "command": "less %f", "keys": "F3" },
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
    "status_warn_bg":      "#ddaa00"
  }
}
```

The values above are the built-in defaults; only include the keys you want to change.

## Related files

Besides `config.json`, sc keeps a few other files under `~/.config/sc/` and
`~/.local/share/sc/`:

| File | Contents |
|---|---|
| `~/.config/sc/config.json` | This file. |
| `~/.config/sc/bookmarks.json` | Bookmarked directories (`C-b` to add, `C-\` to browse). |
| `~/.local/share/sc/state.json` | Panel layout, visibility toggles, and last-visited paths. |
| `~/.local/share/sc/command_history` | Command line history. |
| `~/.local/share/sc/panel_history.json` | Per-panel directory navigation history. |

None of these need to be created or edited by hand under normal use.
