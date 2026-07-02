#![allow(dead_code)]

mod app;
mod bookmarks;
mod cli;
mod config;
mod ipc;
mod panel_history;
mod subshell;
mod history;
mod macros;
mod provider;
mod state;
mod ui;

use app::{resolve_startup_paths, App};
use clap::Parser;
use cli::Cli;
use config::Config;
use state::AppState;
use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut config = Config::load()?;
    if let Some(flag) = cli.subshell_flag() {
        config.startup.subshell = flag;
    }
    let saved_state = AppState::load();
    let (ph_left, ph_right) = panel_history::load();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    let left_ph_path = ph_left.current_path().map(PathBuf::from);
    let right_ph_path = ph_right.current_path().map(PathBuf::from);
    let saved_paths: Option<(&Path, &Path)> = match (&left_ph_path, &right_ph_path) {
        (Some(l), Some(r)) => Some((l.as_path(), r.as_path())),
        _ => None,
    };

    let startup = resolve_startup_paths(
        cli.dir1.as_deref(),
        cli.dir2.as_deref(),
        cli.restore_paths_flag(),
        config.startup.restore_paths,
        saved_paths,
        &cwd,
    );

    let mut app = App::new(config, startup.left, startup.right, &saved_state, ph_left, ph_right, cli.mouse());
    app.run()
}
