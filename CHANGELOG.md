# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).
Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).

## [Unreleased]

### Added

- Asynchronous file search (`Alt-?` / `Ctrl-f`, action `search`): find files by name
  (glob or regex) and optionally by content from the active panel's directory, with
  max-depth / hidden / follow-symlinks options. Hits stream live into a results panel
  shown in place of the active panel (mc-style Enter, tagging, F5/F6/F8 to a normal
  inactive panel, `Alt-r` re-run); content searches replace the inactive panel with a
  matches panel that follows the selection and opens the new full-screen text viewer
  jumped to the matching line. New color-scheme keys `search_match_fg`/`search_match_bg`.
  See docs/FileSearch.md.
- The most recent search a panel jumped away from now stays reachable through that
  panel's back/forward history for the session: `Alt-Left`/`Alt-Right` move into and out
  of it symmetrically in both directions, `Alt-Up` closes it like Esc-Esc, and starting a
  new search (or `Alt-r`) drops whatever was cached. Not persisted to
  `panel_history.json`. A search cached before it finished is marked
  `(partial, Alt-r to refresh)` in its footer.
- `Alt-m` (action `toggle_matches_panel`): show/hide the matches panel of a content
  search without closing the search itself. Works from either panel. Outside a content
  search, warns "The match panel is available only for search by content results"; the
  existing "normal panel as destination" warning for F5/F6 now names this key when the
  inactive panel is the matches panel.

### Changed

- The filter dialog's default binding moved from `Ctrl-f` to `Alt-f` (`Ctrl-f` now
  opens the search dialog).

### Fixed

- sc could crash on startup after a session ended with the active panel's history cursor
  not at the newest entry (e.g. quitting right after `Alt-Left`): the cached-search
  side-vector wasn't padded back into alignment after loading `panel_history.json`
  (which never persists it), so the first navigation of the new session panicked.
- Content search could hang forever on a FIFO with no writer (the search worker blocked
  in `File::open` with no way to be cancelled); content-scanning now skips non-regular
  files. A symlink cycle with "Follow symlinks" enabled could make the walker re-descend
  into itself indefinitely; visited directories are now tracked by canonical path.
- Several gaps in the search-history caching added above: jumping into a hit again from an
  already-restored cached search could leave two caches resident at once; deleting a hit
  from a restored cached search didn't update the cache, so it could resurrect later;
  `Alt-Left` into a cache whose root directory had since been deleted silently restored it
  instead of showing an error; the path-history popup (`Alt-H`) and IPC's `Filter` message
  didn't respect the same "not available on a search panel" guard the equivalent keybound
  actions have; and restoring a cached search didn't re-apply the panel's current sort
  order or clear a stale error banner.
- sc now compiles for aarch64-apple-darwin: added macOS-specific peer credential retrieval in IPC
  (`getsockopt(LOCAL_PEERCRED)` with `xucred` length/version validation), passed openpty's winsize
  argument as the `*mut` pointer macOS expects, and let the TIOCSCTTY ioctl request cast infer each
  platform's type — no behavior change on Linux.
- Pasting into the command line stopped working after the subshell was shown once via
  Ctrl-O — sc never handled bracketed-paste terminal events, and paste mode could be left
  enabled by the subshell's own readline session after returning to the sc UI.
- F8 (delete) on a content search's results panel was refused with "File operations need
  a normal panel as destination" whenever the matches panel was showing — delete has no
  destination, so that check never applied to it. Deleting a hit now also re-syncs the
  matches panel immediately (updated selection, or cleared if none remain) instead of
  waiting for the next tick.

## [0.1.0] - 2026-07-09

Initial release.
