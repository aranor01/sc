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

### Changed

- The filter dialog's default binding moved from `Ctrl-f` to `Alt-f` (`Ctrl-f` now
  opens the search dialog).

### Fixed

- sc now compiles for aarch64-apple-darwin: added macOS-specific peer credential retrieval in IPC
  (`getsockopt(LOCAL_PEERCRED)` with `xucred` length/version validation), passed openpty's winsize
  argument as the `*mut` pointer macOS expects, and let the TIOCSCTTY ioctl request cast infer each
  platform's type — no behavior change on Linux.
- Pasting into the command line stopped working after the subshell was shown once via
  Ctrl-O — sc never handled bracketed-paste terminal events, and paste mode could be left
  enabled by the subshell's own readline session after returning to the sc UI.

## [0.1.0] - 2026-07-09

Initial release.
