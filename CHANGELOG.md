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
- The most recent search a panel jumped away from (via Enter on a hit) stays reachable
  through that panel's ordinary back/forward history for the session: `Alt-Left`/
  `Alt-Right` move into and out of it in both directions, `Alt-Up` always closes it
  outright, and starting a new search (or `Alt-r`) drops whatever was cached along with
  any stale forward history, so `Alt-Right` has nothing left to jump to. Not
  persisted to `panel_history.json`. A search cached before it finished, or interrupted
  in place by `Esc`, is marked `(partial, Alt-r to refresh)` in its footer; `Esc` on a
  still-running search interrupts it first and only closes the results (and matches)
  panel on a second press (or the first press once it's already interrupted or
  complete).
- `Alt-m` (action `toggle_matches_panel`): show/hide the matches panel of a content
  search without closing the search itself. Works from either panel. Outside a content
  search, warns "The match panel is available only for search by content results"; the
  existing "normal panel as destination" warning for F5/F6 now names this key when the
  inactive panel is the matches panel.
- Running a command (from the command line or via `Ctrl-o`/subshell passthrough) while
  the matches panel is the active panel uses the directory of the file whose matches
  are shown as the working directory, since the matches panel isn't a directory browser.

### Changed

- The filter dialog's default binding moved from `Ctrl-f` to `Alt-f` (`Ctrl-f` now
  opens the search dialog).

### Fixed

- sc now compiles and runs on aarch64-apple-darwin (macOS peer-credential retrieval in IPC,
  portable ioctl/winsize types) — no behavior change on Linux.
- Pasting into the command line stopped working after the subshell was shown once via
  Ctrl-O.
- Navigating into a directory via Alt-i (open the active panel's directory in the other
  panel) or a mouse double-click didn't record the destination in that panel's own
  back/forward history, so `Alt-Left`/`Alt-Right` could subsequently land on unrelated
  directories or skip a level.
- Filter/select-group/unselect-group dialogs now show their validation error at the
  bottom of the dialog.

## [0.1.0] - 2026-07-09

Initial release.
