use std::path::Path;

/// Abbreviates the home directory prefix as `~` for display; leaves other paths unchanged.
pub fn abbreviate(path: &str) -> String {
    abbreviate_with(path, dirs::home_dir().as_deref())
}

pub fn abbreviate_with(path: &str, home: Option<&Path>) -> String {
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

/// Expands a leading `~` (or `~/...`) back to the home directory; leaves other paths unchanged.
pub fn expand(path: &str) -> String {
    expand_with(path, dirs::home_dir().as_deref())
}

pub fn expand_with(path: &str, home: Option<&Path>) -> String {
    let Some(home) = home else { return path.to_string(); };
    if path == "~" {
        return home.to_string_lossy().into_owned();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{}/{rest}", home.to_string_lossy());
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviate_exact_match() {
        let r = abbreviate_with("/home/alice", Some(Path::new("/home/alice")));
        assert_eq!(r, "~");
    }

    #[test]
    fn abbreviate_subdir() {
        let r = abbreviate_with("/home/alice/projects", Some(Path::new("/home/alice")));
        assert_eq!(r, "~/projects");
    }

    #[test]
    fn abbreviate_respects_segment_boundary() {
        let r = abbreviate_with("/home/alice2/projects", Some(Path::new("/home/alice")));
        assert_eq!(r, "/home/alice2/projects");
    }

    #[test]
    fn abbreviate_unrelated_path_unchanged() {
        let r = abbreviate_with("/var/log", Some(Path::new("/home/alice")));
        assert_eq!(r, "/var/log");
    }

    #[test]
    fn abbreviate_no_home_dir_unchanged() {
        let r = abbreviate_with("/var/log", None);
        assert_eq!(r, "/var/log");
    }

    #[test]
    fn expand_tilde_alone() {
        let r = expand_with("~", Some(Path::new("/home/alice")));
        assert_eq!(r, "/home/alice");
    }

    #[test]
    fn expand_tilde_subdir() {
        let r = expand_with("~/projects", Some(Path::new("/home/alice")));
        assert_eq!(r, "/home/alice/projects");
    }

    #[test]
    fn expand_non_tilde_path_unchanged() {
        let r = expand_with("/var/log", Some(Path::new("/home/alice")));
        assert_eq!(r, "/var/log");
    }

    #[test]
    fn expand_no_home_dir_unchanged() {
        let r = expand_with("~/projects", None);
        assert_eq!(r, "~/projects");
    }

    #[test]
    fn roundtrip() {
        let home = Some(Path::new("/home/alice"));
        let original = "/home/alice/projects/sc";
        let abbreviated = abbreviate_with(original, home);
        assert_eq!(expand_with(&abbreviated, home), original);
    }
}
