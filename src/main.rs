#![allow(dead_code)]

mod app;
mod bookmarks;
mod config;
mod panel_history;
mod subshell;
mod history;
mod macros;
mod provider;
mod state;
mod ui;

use app::{resolve_startup_paths, App};
use config::Config;
use state::AppState;
use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut dir1: Option<PathBuf> = None;
    let mut dir2: Option<PathBuf> = None;
    let mut restore_flag: Option<bool> = None;
    let mut mouse = true;
    let mut subshell_flag: Option<bool> = None;

    for arg in &args {
        match arg.as_str() {
            "--restore-paths" => restore_flag = Some(true),
            "--no-restore-paths" => restore_flag = Some(false),
            "--subshell" => subshell_flag = Some(true),
            "--no-subshell" => subshell_flag = Some(false),
            "-d" | "--nomouse" => mouse = false,
            s if !s.starts_with('-') => {
                if dir1.is_none() {
                    dir1 = Some(PathBuf::from(s));
                } else if dir2.is_none() {
                    dir2 = Some(PathBuf::from(s));
                }
            }
            _ => {}
        }
    }

    let mut config = Config::load()?;
    if let Some(flag) = subshell_flag {
        config.startup.subshell = flag;
    }
    let saved_state = AppState::load();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    let saved_paths = Some((
        Path::new(&saved_state.left_path),
        Path::new(&saved_state.right_path),
    ));

    let startup = resolve_startup_paths(
        dir1.as_deref(),
        dir2.as_deref(),
        restore_flag,
        config.startup.restore_paths,
        saved_paths,
        &cwd,
    );

    let mut app = App::new(config, startup.left, startup.right, &saved_state, mouse);
    app.run()
}
