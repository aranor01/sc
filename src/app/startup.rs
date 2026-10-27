use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct StartupPaths {
    pub left: PathBuf,
    pub right: PathBuf,
}

pub fn resolve_startup_paths(
    dir1: Option<&Path>,
    dir2: Option<&Path>,
    flag: Option<bool>,
    restore_paths_config: bool,
    saved: Option<(&Path, &Path)>,
    cwd: &Path,
) -> StartupPaths {
    if let Some(d1) = dir1 {
        let right = dir2.unwrap_or(d1);
        return StartupPaths {
            left: absolutize(d1, cwd),
            right: absolutize(right, cwd),
        };
    }
    let restore = flag.unwrap_or(restore_paths_config);
    if restore {
        if let Some((left, right)) = saved {
            return StartupPaths {
                left: absolutize(left, cwd),
                right: absolutize(right, cwd),
            };
        }
    }
    StartupPaths {
        left: cwd.to_path_buf(),
        right: cwd.to_path_buf(),
    }
}

pub(super) fn absolutize(path: &Path, cwd: &Path) -> PathBuf {
    let joined = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    normalize_lexical(&joined)
}

/// Lexically collapses `.` and `..` components. Deliberately not `fs::canonicalize`: that
/// requires the path to exist (breaks the deleted-directory corner case) and resolves
/// symlinks, which would surprise a user navigating through one.
pub(super) fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Abbreviates the home directory prefix as `~` for display; leaves other paths unchanged.
pub(super) fn display_path(path: &str) -> String {
    abbreviate_home(path, dirs::home_dir().as_deref())
}

pub(super) fn abbreviate_home(path: &str, home: Option<&Path>) -> String {
    let Some(home) = home else { return path.to_string(); };
    let home = home.to_string_lossy();
    if path == home.as_ref() {
        return "~".to_string();
    }
    if let Some(rest) = path.strip_prefix(home.as_ref()).and_then(|r| r.strip_prefix('/')) {
        return format!("~/{rest}");
    }
    path.to_string()
}

/// Like `absolutize`, but joins against the process cwd for call sites with no `cwd` on hand.
pub(super) fn absolutize_str(path: &str) -> String {
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        cwd.join(p)
    };
    normalize_lexical(&joined).to_string_lossy().into_owned()
}
