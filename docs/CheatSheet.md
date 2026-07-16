# Sunset Commander — Key Bindings Cheat Sheet

Config keys refer to the field names in the `keybindings` section of `~/.config/sc/config.json`.
Defaults use the notation accepted by the config parser (e.g. `Alt-Up`, `Ctrl-s`, `Ctrl-x t`).

## Modifier Keys

`Alt-`, `Ctrl-`, and `Shift-` are combinable and order-independent, e.g. `Ctrl-Alt-b`. The
config parser also accepts the short forms `A-`, `C-`, `S-` — see
[`Configuration.md`](Configuration.md#key-spec-syntax).

---

## Directory Navigation

| Action | Description | Config key: default |
|---|---|---|
| Go to Parent | Navigate to the parent directory of the active panel | `go_to_parent`: `Alt-Up` |
| Go Back | Go back to the previous directory in the active panel's history | `go_back`: `Alt-Left` |
| Go Forward | Go forward in the active panel's history | `go_forward`: `Alt-Right` |
| Quick Search | Jump to the first entry whose name starts with the typed string (case-insensitive prefix) | `quicksearch`: `/`, `Alt-s` |
| Filter | Open a dialog to hide entries not matching a pattern (shell glob by default; dialog offers RegExp, Files only, and Case sensitive options); empty pattern removes the filter | `filter`: `Alt-f` |
| Search | Find files by name and optionally content, asynchronously; hits stream into a results panel (see [`FileSearch.md`](FileSearch.md)) | `search`: `Alt-?`, `Ctrl-f` |
| Toggle Matches Panel | Show/hide the matches panel of a content search without closing it (only applies to content-search results) | `toggle_matches_panel`: `Alt-m` |
| Toggle Hidden Files | Toggle visibility of dotfiles in the active panel | `toggle_hidden`: `Alt-.` |
| Directory History | Open path history popup for the active panel (most recent first) | `path_history`: `Alt-H`, `Alt-Down` |

## Panels

| Action | Description | Config key: default |
|---|---|---|
| Switch Panel | Switch focus to the other panel | `switch_panel`: `Tab` |
| Sort Panel | Open sort popup: choose sort order (Name, Extension, Size, Modified, Unsorted; asc/desc) | `sort_panel`: `Ctrl-s` |
| Sync Panels | Open in the inactive panel the same directory shown in the active panel | `sync_panels`: `Alt-i` |
| Refresh Panel | Force-refresh the active panel's directory listing (re-reads from disk) | `refresh_panel`: `Alt-r` |

## General Navigation and Interaction Keys (non-configurable)

| Action | Description | Key(s) |
|---|---|---|
| Move Up | Move cursor up one entry | `Up` |
| Move Down | Move cursor down one entry | `Down` |
| Page Up | Scroll cursor up one full page | `Page Up` |
| Page Down | Scroll cursor down one full page | `Page Down` |
| Jump to First | Jump to the first entry | `Home` |
| Jump to Last | Jump to the last entry | `End` |
| Open / Execute | Enter the selected directory; or run the command line if it contains text | `Enter` |
| Cancel / Focus Panel | Dismiss dialogs and prompts; or give panels temporary focus when command line has text | `Esc` |

## Bookmarks

| Action | Description | Config key: default |
|---|---|---|
| Open Bookmarks | Open bookmarks popup to navigate the active panel to a bookmarked directory | `bookmark_open`: `Ctrl-\` |
| Add Bookmark | Add the active panel's current directory to bookmarks | `bookmark_add`: `Ctrl-b` |
| Remove Bookmark | While the bookmarks popup is open, remove the currently selected entry | `Del` |

## File Selection

| Action | Description | Config key: default |
|---|---|---|
| Tag File | Tag or untag the currently selected file and move to the next entry | `tag_file`: `Ins` |
| Invert Tags | Tag all untagged and untag all tagged entries in the current panel; if the command line has text, appends `*` to it instead | `invert_tags`: `*` |
| Select Group | Open a dialog to tag all visible entries matching a pattern (shell glob by default; dialog offers RegExp, Files only, and Case sensitive options) | `select_group`: `+` |
| Unselect Group | Open a dialog to untag all visible entries matching a pattern (same options as Select Group) | `unselect_group`: `-` |

## File Operations

| Action | Description | Config key: default |
|---|---|---|
| Copy | Copy tagged files (or the selected file) to the inactive panel's directory | `copy`: `F5` |
| Move | Move tagged files (or the selected file) to the inactive panel's directory | `move_entry`: `F6` |
| Delete | Delete the tagged files (or the selected file) in the active panel | `delete`: `F8` |
| Rename | Rename the currently selected file | `rename`: `Shift-F6` |
| Make Directory | Create a new directory in the active panel | `mkdir`: `F7` |

## Command Line

| Action | Description | Config key: default |
|---|---|---|
| Insert Filename | Copy the selected file name to the command line | `cmdline_insert_filename`: `Alt-Enter`, `Ctrl-Enter` |
| Insert Full Path | Copy the full path of the selected file to the command line | `cmdline_insert_fullpath`: `Ctrl-Shift-Enter` |
| Autocomplete | Autocomplete filenames, commands, variables, usernames, and hostnames | `cmdline_complete`: `Alt-Tab`, `Ctrl-Space` |
| Insert Tagged (Active) | Copy tagged files of the active panel to the command line | `cmdline_insert_tagged`: `Ctrl-x t` |
| Insert Tagged (Inactive) | Copy tagged files of the inactive panel to the command line | `cmdline_insert_tagged_other`: `Ctrl-x Ctrl-t` |
| Insert Path (Active) | Copy the active panel's directory path to the command line | `cmdline_insert_path`: `Ctrl-x p` |
| Insert Path (Inactive) | Copy the inactive panel's directory path to the command line | `cmdline_insert_path_other`: `Ctrl-x Ctrl-p` |
| History Previous | Go to the previous command in the history | `cmdline_history_prev`: `Ctrl-Up` |
| History Next | Go to the next command in the history | `cmdline_history_next`: `Ctrl-Down` |
| Reverse Search | Filter command history by the current command line text, showing matches above the command line (most recent highlighted, Backspace re-filters); Enter/Tab accepts the highlighted entry, Esc cancels | `reverse_search`: `Ctrl-r`, `Alt-h` |

## Shell

| Action | Description | Config key: default |
|---|---|---|
| Toggle Shell | Toggle output overlay (stateless mode) or full interactive subshell (subshell mode); Ctrl-o and Alt-o also exit subshell passthrough back to the sc UI | `toggle_shell`: `Ctrl-o` |
| Toggle Shell & Sync | Like Toggle Shell; also copies the SC command line into the subshell's readline buffer | `toggle_shell_and_sync_command_line`: `Alt-o` |

## Layout

| Action | Description | Config key: default |
|---|---|---|
| Toggle Layout | Toggle panel layout between vertical and horizontal | `toggle_layout`: `Alt-,` |
| Toggle Command Line | Toggle command line visibility | `toggle_cmdline`: `Ctrl-Alt-b` |
| Toggle Button Bar | Toggle button bar visibility | `toggle_button_bar`: `Alt-b` |

## Miscellaneous

| Action | Description | Config key: default |
|---|---|---|
| User Menu | Open the user menu | `user_menu`: `F2` |
| Exit | Exit the application | `exit`: `F10`, `Ctrl-q` |
