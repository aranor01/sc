# Sunset Commander — Key Bindings Cheat Sheet

Config keys refer to the field names in the `keybindings` section of `~/.config/sc/config.json`.
Defaults use the notation accepted by the config parser (e.g. `A-Up`, `C-s`, `C-x t`).

---

## Directory Navigation

| Action | Description | Config key: default |
|---|---|---|
| Go to Parent | Navigate to the parent directory of the active panel | `go_to_parent`: `A-Up` |
| Go Back | Go back to the previous directory in the active panel's history | `go_back`: `A-Left` |
| Go Forward | Go forward in the active panel's history | `go_forward`: `A-Right` |
| Quick Search | Jump to the first entry whose name starts with the typed string (case-insensitive prefix) | `quicksearch`: `/`, `A-s` |
| Filter | Hide entries not matching a pattern (glob or `/`-prefixed regex); empty pattern removes filter | `filter`: `C-f` |
| Toggle Hidden Files | Toggle visibility of dotfiles in the active panel | `toggle_hidden`: `A-.` |
| Directory History | Open path history popup for the active panel (most recent first) | `path_history`: `A-H`, `A-Down` |

## Panels

| Action | Description | Config key: default |
|---|---|---|
| Switch Panel | Switch focus to the other panel | `switch_panel`: `Tab` |
| Sort Panel | Open sort popup: choose sort order (Name, Extension, Size, Modified, Unsorted; asc/desc) | `sort_panel`: `C-s` |
| Sync Panels | Open in the inactive panel the same directory shown in the active panel | `sync_panels`: `A-i` |
| Refresh Panel | Force-refresh the active panel's directory listing (re-reads from disk) | `refresh_panel`: `A-r` |

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
| Open Bookmarks | Open bookmarks popup to navigate the active panel to a bookmarked directory | `bookmark_open`: `C-\` |
| Add Bookmark | Add the active panel's current directory to bookmarks | `bookmark_add`: `C-b` |
| Remove Bookmark | While the bookmarks popup is open, remove the currently selected entry | `Del` |

## File Selection

| Action | Description | Config key: default |
|---|---|---|
| Tag File | Tag or untag the currently selected file and move to the next entry | `tag_file`: `Ins` |
| Invert Tags | Tag all untagged and untag all tagged entries in the current panel | `invert_tags`: `*` |
| Select Group | Tag all visible entries matching a pattern (glob or `/`-prefixed regex) | `select_group`: `+` |
| Unselect Group | Untag all visible entries matching a pattern (glob or `/`-prefixed regex) | `unselect_group`: `-` |

## File Operations

| Action | Description | Config key: default |
|---|---|---|
| Copy | Copy tagged files (or the selected file) to the inactive panel's directory | `copy`: `F5` |
| Move | Move tagged files (or the selected file) to the inactive panel's directory | `move_entry`: `F6` |
| Delete | Delete the tagged files (or the selected file) in the active panel | `delete`: `F8` |
| Rename | Rename the currently selected file | `rename`: `S-F6` |
| Make Directory | Create a new directory in the active panel | `mkdir`: `F7` |

## Command Line

| Action | Description | Config key: default |
|---|---|---|
| Insert Filename | Copy the selected file name to the command line | `cmdline_insert_filename`: `A-Enter`, `C-Enter` |
| Insert Full Path | Copy the full path of the selected file to the command line | `cmdline_insert_fullpath`: `C-S-Enter` |
| Autocomplete | Autocomplete filenames, commands, variables, usernames, and hostnames | `cmdline_complete`: `A-Tab`, `C-Space` |
| Insert Tagged (Active) | Copy tagged files of the active panel to the command line | `cmdline_insert_tagged`: `C-x t` |
| Insert Tagged (Inactive) | Copy tagged files of the inactive panel to the command line | `cmdline_insert_tagged_other`: `C-x C-t` |
| Insert Path (Active) | Copy the active panel's directory path to the command line | `cmdline_insert_path`: `C-x p` |
| Insert Path (Inactive) | Copy the inactive panel's directory path to the command line | `cmdline_insert_path_other`: `C-x C-p` |
| History Previous | Go to the previous command in the history | `cmdline_history_prev`: `C-Up` |
| History Next | Go to the next command in the history | `cmdline_history_next`: `C-Down` |
| Reverse Search | Filter command history by current command line text; Enter/Tab accepts, Esc cancels | `reverse_search`: `C-r`, `A-h` |

## Shell

| Action | Description | Config key: default |
|---|---|---|
| Toggle Shell | Toggle output overlay (stateless mode) or full interactive subshell (subshell mode) | `toggle_shell`: `C-o` |
| Toggle Shell & Sync | Like Toggle Shell; also copies the SC command line into the subshell's readline buffer | `toggle_shell_and_sync_command_line`: `A-o` |

## Layout

| Action | Description | Config key: default |
|---|---|---|
| Toggle Layout | Toggle panel layout between vertical and horizontal | `toggle_layout`: `A-,` |
| Toggle Command Line | Toggle command line visibility | `toggle_cmdline`: `C-A-b` |
| Toggle Button Bar | Toggle button bar visibility | `toggle_button_bar`: `A-b` |

## Miscellaneous

| Action | Description | Config key: default |
|---|---|---|
| User Menu | Open the user menu | `user_menu`: `F2` |
| Exit | Exit the application | `exit`: `F10`, `C-q` |
