# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).
Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).

## [Unreleased]

### Fixed

- sc now compiles for aarch64-apple-darwin: added macOS-specific peer credential retrieval in IPC,
  and cfg-gated libc call sites (openpty's winsize pointer, TIOCSCTTY's request type) to match
  each platform's stricter signature requirements without changing behavior on Linux.
- Pasting into the command line stopped working after the subshell was shown once via
  Ctrl-O — sc never handled bracketed-paste terminal events, and paste mode could be left
  enabled by the subshell's own readline session after returning to the sc UI.

## [0.1.0] - 2026-07-09

Initial release.
