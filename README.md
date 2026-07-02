# Sunset Commander (sc)

A two-panel, keyboard- and mouse-driven visual shell for Linux terminals, in the spirit of
Midnight Commander.

## Features

- Two panels for browsing directories side by side; one panel is active at a time.
- Toggle the panel layout between vertical (side by side) and horizontal (stacked).
- Multiple file selection (tagging), including pattern-based select/unselect groups.
- Copy or move the selected/tagged files in the active panel to the directory shown in the
  other panel; delete from the active panel.
- Rename, create directories, sort panels by name/extension/size/modified time.
- Directory bookmarks and per-panel navigation history.
- Quicksearch and filtering within a panel.
- A command line below the panels, with history, reverse search, and bash-style completion.
- A configurable user menu of shell commands, with macro substitution for file/path
  arguments (see [`docs/Configuration.md`](docs/Configuration.md)).
- A button bar showing the active function-key bindings.
- Two ways to run shell commands: a stateless mode (one-off commands with a scrollable
  output overlay) and a subshell mode (a persistent shell session you can drop into).
- Fully configurable key bindings, color scheme, and startup behavior via a JSON config file.
- Mouse support: click to select, double-click to open, right-click to tag.

## Installation

Requires a recent Rust toolchain ([rustup.rs](https://rustup.rs)).

Build and run from source:

```sh
cargo build --release
./target/release/sc
```

Or build a Debian package with [`cargo-deb`](https://github.com/kornelski/cargo-deb):

```sh
cargo install cargo-deb
SC_INSTALL_PREFIX=/usr cargo deb
sudo dpkg -i target/debian/sc_*.deb
```

## Usage

```sh
sc [OPTIONS] [DIR1] [DIR2]
```

Run `sc --help` for the full list of options, or see
[`docs/CommandLineArgs.md`](docs/CommandLineArgs.md) for the complete reference, including
how `DIR1`/`DIR2` interact with the `--restore-paths` option.

## Key bindings

All default key bindings, with a description of each, are listed in
[`docs/CheatSheet.md`](docs/CheatSheet.md). Every binding listed there is rebindable in the
config file.

## Configuration

sc reads an optional JSON config file at `~/.config/sc/config.json` covering key bindings,
the color scheme, the user menu, and startup behavior. See
[`docs/Configuration.md`](docs/Configuration.md) for the full reference, and
[`docs/MacroSubstitution.md`](docs/MacroSubstitution.md) for the macros available in user
menu commands.

## License

MIT — see [`LICENSE`](LICENSE).
