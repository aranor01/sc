use std::path::PathBuf;

use clap::Parser;

/// sc — a visual shell for Linux terminals, in the spirit of Midnight Commander.
#[derive(Parser, Debug)]
#[command(name = "sc", version, about, long_about = None)]
pub struct Cli {
    /// Starting path for both panels
    #[arg(value_name = "DIR1")]
    pub dir1: Option<PathBuf>,

    /// Starting path for the right panel only (requires DIR1)
    #[arg(value_name = "DIR2")]
    pub dir2: Option<PathBuf>,

    /// Restore panel paths from the last session, overriding the config file
    #[arg(long, conflicts_with = "no_restore_paths")]
    restore_paths: bool,

    /// Start panels at the current working directory, overriding the config file
    #[arg(long, conflicts_with = "restore_paths")]
    no_restore_paths: bool,

    /// Start in subshell mode, overriding the config file
    #[arg(long, conflicts_with = "no_subshell")]
    subshell: bool,

    /// Start in stateless mode, overriding the config file
    #[arg(long, conflicts_with = "subshell")]
    no_subshell: bool,

    /// Enable IPC actions beyond ShowPanels, overriding the config file
    #[arg(long, conflicts_with = "no_ipc_scripting")]
    ipc_scripting: bool,

    /// Disable IPC actions beyond ShowPanels, overriding the config file
    #[arg(long, conflicts_with = "ipc_scripting")]
    no_ipc_scripting: bool,

    /// Disable mouse support
    #[arg(short = 'd', long = "no-mouse")]
    no_mouse: bool,
}

impl Cli {
    /// Collapses `--restore-paths` / `--no-restore-paths` into a single override.
    pub fn restore_paths_flag(&self) -> Option<bool> {
        if self.restore_paths {
            Some(true)
        } else if self.no_restore_paths {
            Some(false)
        } else {
            None
        }
    }

    /// Collapses `--subshell` / `--no-subshell` into a single override.
    pub fn subshell_flag(&self) -> Option<bool> {
        if self.subshell {
            Some(true)
        } else if self.no_subshell {
            Some(false)
        } else {
            None
        }
    }

    /// Collapses `--ipc-scripting` / `--no-ipc-scripting` into a single override.
    pub fn ipc_scripting_flag(&self) -> Option<bool> {
        if self.ipc_scripting {
            Some(true)
        } else if self.no_ipc_scripting {
            Some(false)
        } else {
            None
        }
    }

    pub fn mouse(&self) -> bool {
        !self.no_mouse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_has_no_overrides() {
        let cli = Cli::parse_from(["sc"]);
        assert_eq!(cli.dir1, None);
        assert_eq!(cli.dir2, None);
        assert_eq!(cli.restore_paths_flag(), None);
        assert_eq!(cli.subshell_flag(), None);
        assert_eq!(cli.ipc_scripting_flag(), None);
        assert!(cli.mouse());
    }

    #[test]
    fn positional_dirs_are_parsed() {
        let cli = Cli::parse_from(["sc", "/tmp", "/var"]);
        assert_eq!(cli.dir1, Some(PathBuf::from("/tmp")));
        assert_eq!(cli.dir2, Some(PathBuf::from("/var")));
    }

    #[test]
    fn only_dir1_leaves_dir2_none() {
        let cli = Cli::parse_from(["sc", "/tmp"]);
        assert_eq!(cli.dir1, Some(PathBuf::from("/tmp")));
        assert_eq!(cli.dir2, None);
    }

    #[test]
    fn restore_paths_flag_true() {
        let cli = Cli::parse_from(["sc", "--restore-paths"]);
        assert_eq!(cli.restore_paths_flag(), Some(true));
    }

    #[test]
    fn restore_paths_flag_false() {
        let cli = Cli::parse_from(["sc", "--no-restore-paths"]);
        assert_eq!(cli.restore_paths_flag(), Some(false));
    }

    #[test]
    fn conflicting_restore_flags_error() {
        let result = Cli::try_parse_from(["sc", "--restore-paths", "--no-restore-paths"]);
        assert!(result.is_err());
    }

    #[test]
    fn subshell_flag_true() {
        let cli = Cli::parse_from(["sc", "--subshell"]);
        assert_eq!(cli.subshell_flag(), Some(true));
    }

    #[test]
    fn subshell_flag_false() {
        let cli = Cli::parse_from(["sc", "--no-subshell"]);
        assert_eq!(cli.subshell_flag(), Some(false));
    }

    #[test]
    fn conflicting_subshell_flags_error() {
        let result = Cli::try_parse_from(["sc", "--subshell", "--no-subshell"]);
        assert!(result.is_err());
    }

    #[test]
    fn ipc_scripting_flag_true() {
        let cli = Cli::parse_from(["sc", "--ipc-scripting"]);
        assert_eq!(cli.ipc_scripting_flag(), Some(true));
    }

    #[test]
    fn ipc_scripting_flag_false() {
        let cli = Cli::parse_from(["sc", "--no-ipc-scripting"]);
        assert_eq!(cli.ipc_scripting_flag(), Some(false));
    }

    #[test]
    fn conflicting_ipc_scripting_flags_error() {
        let result = Cli::try_parse_from(["sc", "--ipc-scripting", "--no-ipc-scripting"]);
        assert!(result.is_err());
    }

    #[test]
    fn no_mouse_short_and_long_flag() {
        assert!(!Cli::parse_from(["sc", "-d"]).mouse());
        assert!(!Cli::parse_from(["sc", "--no-mouse"]).mouse());
        assert!(Cli::parse_from(["sc"]).mouse());
    }

    #[test]
    fn unknown_flag_errors() {
        let result = Cli::try_parse_from(["sc", "--bogus"]);
        assert!(result.is_err());
    }

    #[test]
    fn too_many_positional_args_errors() {
        let result = Cli::try_parse_from(["sc", "/a", "/b", "/c"]);
        assert!(result.is_err());
    }

    #[test]
    fn help_flag_is_recognized() {
        let result = Cli::try_parse_from(["sc", "--help"]);
        assert_eq!(result.unwrap_err().kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn version_flag_is_recognized() {
        let result = Cli::try_parse_from(["sc", "--version"]);
        assert_eq!(result.unwrap_err().kind(), clap::error::ErrorKind::DisplayVersion);
    }
}
