# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).
Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).

## [Unreleased]

### Added

- Asynchronous file search (`Alt-?` / `Ctrl-f`, action `search`): find files by name
  (glob or regex) and optionally by content from the active panel's directory, with
  max-depth / hidden / follow-symlinks options. **File pattern** has its own **RegExp**/
  **Case sensitive** checkboxes directly beneath it; **Containing text** has its own
  independent **RegExp**/**Case sensitive**/**Whole words** checkboxes, so content
  searches support regex and whole-word matching in addition to plain substrings. Hits
  stream live into a results panel shown in place of the active panel (mc-style Enter,
  tagging, F5/F6/F8 to a normal inactive panel, `Alt-r` re-run); content searches
  replace the inactive panel with a matches panel that follows the selection and opens
  the new full-screen text viewer jumped to the matching line — both consistently honor
  the content search's regex/whole-word mode when highlighting. Lines too long to fit
  are truncated around the first match (marked with `~`) so it stays visible instead of
  being cut off. New color-scheme keys `search_match_fg`/`search_match_bg`. See
  docs/FileSearch.md.
- A panel's most recent search stays reachable via its own back/forward history for the
  session (`Alt-Left`/`Alt-Right`/`Alt-Up`), and `Esc` on a running search interrupts it
  in place before a second `Esc` closes it. See docs/FileSearch.md.
- `Alt-m` (action `toggle_matches_panel`): show/hide the matches panel of a content
  search without closing the search itself. Works from either panel. Outside a content
  search, warns "The match panel is available only for search by content results"; the
  existing "normal panel as destination" warning for F5/F6 now names this key when the
  inactive panel is the matches panel.
- Running a command (from the command line or via `Ctrl-o`/subshell passthrough) while
  the matches panel is the active panel uses the directory of the file whose matches
  are shown as the working directory, since the matches panel isn't a directory browser.
- `F3` (action `view`): opens the internal full-screen text viewer on the active panel's
  current entry. The bundled default menu's external "View" entry moves to `Shift-F3`.
- Enter/double-click on a file now runs `panels.default_action_executable` or
  `panels.default_action_text` (falling back to `panels.default_action`), defaulting to
  opening the internal viewer for non-executable files and doing nothing otherwise.

### Changed

- The filter dialog's default binding moved from `Ctrl-f` to `Alt-f` (`Ctrl-f` now
  opens the search dialog).

### Fixed

- sc now compiles and runs on aarch64-apple-darwin (macOS peer-credential retrieval in IPC,
  portable ioctl/winsize types) — no behavior change on Linux.
- Pasting into the command line stopped working after the subshell was shown once via
  Ctrl-O.
- Alt-i (open the active panel's directory in the other panel) and mouse double-click
  navigation now record the destination in that panel's own back/forward history.
- Filter/select-group/unselect-group dialogs now show their validation error at the
  bottom of the dialog.
- The command output overlay now resets its scroll position to the top each time a new
  command runs.

## [0.1.0] - 2026-07-09

Initial release.
