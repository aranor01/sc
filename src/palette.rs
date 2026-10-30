//! Command palette data: the command list parsed from `CheatSheet.md` and
//! the recent-actions list. The UI session that uses them lives in `app.rs`, since it
//! needs the private `Action` enum.

/// Category shown for the user-configured menu commands.
pub const USER_CATEGORY: &str = "User Commands";

const EMBEDDED_CHEATSHEET: &str = include_str!("../docs/CheatSheet.md");
const MAX_RECENT: usize = 4;

/// One built-in command parsed from a configurable row of a CheatSheet table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub category: String,
    pub label: String,
    pub description: String,
    /// Name of the `keybindings` field in the config file (e.g. `sort_panel`).
    pub config_key: String,
}

/// Commands from the installed `CheatSheet.md`, or the copy embedded in the binary when
/// the installed one is missing or unreadable.
pub fn load_builtin_commands() -> Vec<Command> {
    let installed = crate::config::find_cheatsheet()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|md| parse_cheatsheet(&md))
        .filter(|c| !c.is_empty());
    installed.unwrap_or_else(|| parse_cheatsheet(EMBEDDED_CHEATSHEET))
}

/// Parses the `## Category` tables. Only rows whose last column reads
/// `` `config_key`: `default`, ... `` are commands; rows listing bare keys
/// (non-configurable) are skipped because they cannot be executed by name.
pub fn parse_cheatsheet(md: &str) -> Vec<Command> {
    let mut category = String::new();
    let mut out = Vec::new();
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            category = h.trim().to_string();
            continue;
        }
        let line = line.trim();
        if category.is_empty() || !line.starts_with('|') {
            continue;
        }
        let cols: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cols.len() != 3 {
            continue;
        }
        let Some(key) = config_key_of(cols[2]) else { continue };
        out.push(Command {
            category: category.clone(),
            label: cols[0].to_string(),
            description: cols[1].to_string(),
            config_key: key,
        });
    }
    out
}

fn config_key_of(cell: &str) -> Option<String> {
    let rest = cell.strip_prefix('`')?;
    let (name, after) = rest.split_once('`')?;
    if !after.starts_with(':') || name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '.') {
        return None;
    }
    Some(name.to_string())
}

/// The part of a description before the first `(`, `;` or `.`, trimmed.
pub fn short_description(desc: &str) -> &str {
    let end = desc.find(['(', ';', '.']).unwrap_or(desc.len());
    desc[..end].trim()
}

/// Moves `id` to the front of `recent`, de-duplicating and capping the list.
pub fn push_recent(recent: &mut Vec<String>, id: &str) {
    recent.retain(|r| r != id);
    recent.insert(0, id.to_string());
    recent.truncate(MAX_RECENT);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Title
| not | a | command |
## Panels
| Action | Description | Config key: default |
|---|---|---|
| Sort Panel | Open sort popup | `sort_panel`: `Ctrl-s` |
| Refresh | Reload | `refresh_panel`: `Alt-r`, `F9` |
## Text Viewer
| Action | Description | Config key: default |
|---|---|---|
| Line Numbers | Show numbers | `viewer.toggle_line_numbers`: `Alt-n` |
## Keys (non-configurable)
| Action | Description | Key(s) |
|---|---|---|
| Move Up | Up | `Up` |
| Remove Bookmark | While popup open | `Del` |
";

    #[test]
    fn parses_only_configurable_rows_with_their_category() {
        let cmds = parse_cheatsheet(SAMPLE);
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0], Command {
            category: "Panels".into(),
            label: "Sort Panel".into(),
            description: "Open sort popup".into(),
            config_key: "sort_panel".into(),
        });
        assert_eq!(cmds[1].config_key, "refresh_panel");
    }

    #[test]
    fn dotted_keys_of_nested_config_objects_are_parsed_in_their_own_category() {
        let cmds = parse_cheatsheet(SAMPLE);
        let c = cmds.iter().find(|c| c.config_key == "viewer.toggle_line_numbers").unwrap();
        assert_eq!(c.category, "Text Viewer");
    }

    #[test]
    fn embedded_cheatsheet_has_commands_in_several_categories() {
        let cmds = parse_cheatsheet(EMBEDDED_CHEATSHEET);
        assert!(cmds.iter().any(|c| c.config_key == "copy" && c.category == "File Operations"));
        assert!(cmds.iter().any(|c| c.config_key == "action_palette"));
        assert!(!cmds.iter().any(|c| c.label == "Move Up"));
    }

    #[test]
    fn short_description_stops_at_first_paren_semicolon_or_dot() {
        assert_eq!(short_description("Open sort popup: choose order (Name, Size; asc)"), "Open sort popup: choose order");
        assert_eq!(short_description("Toggle layout; also more"), "Toggle layout");
        assert_eq!(short_description("Reload. Then more"), "Reload");
        assert_eq!(short_description("No stop chars"), "No stop chars");
        assert_eq!(short_description("(all parenthesised)"), "");
    }

    #[test]
    fn push_recent_dedups_moves_to_front_and_caps() {
        let mut r = Vec::new();
        for i in 0..12 {
            push_recent(&mut r, &format!("a{i}"));
        }
        assert_eq!(r.len(), MAX_RECENT);
        assert_eq!(r[0], "a11");
        push_recent(&mut r, "a5");
        assert_eq!(r[0], "a5");
        assert_eq!(r.iter().filter(|x| *x == "a5").count(), 1);
    }
}
