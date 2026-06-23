use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::path::Path;

const DEFAULT_MAX_LEN: usize = 500;

pub struct CommandHistory {
    entries: VecDeque<String>,
    max_len: usize,
    /// Index into `entries` while navigating (0 = oldest, len-1 = newest).
    /// `None` means the user is not currently navigating history.
    cursor: Option<usize>,
    /// The cmdline text that was present when navigation started.
    draft: String,
}

impl CommandHistory {
    pub fn new() -> Self {
        Self::with_max_len(DEFAULT_MAX_LEN)
    }

    pub fn with_max_len(max_len: usize) -> Self {
        CommandHistory {
            entries: VecDeque::new(),
            max_len,
            cursor: None,
            draft: String::new(),
        }
    }

    /// Number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Add a command to history.
    ///
    /// - Consecutive duplicates are silently dropped.
    /// - Entries exceeding `max_len` are pruned from the oldest end.
    /// - Navigation cursor is reset.
    pub fn push(&mut self, cmd: String) {
        self.reset_cursor();
        if cmd.is_empty() {
            return;
        }
        if self.entries.back().map(|s| s == &cmd).unwrap_or(false) {
            return;
        }
        self.entries.push_back(cmd);
        while self.entries.len() > self.max_len {
            self.entries.pop_front();
        }
    }

    /// Move to the previous (older) command.
    ///
    /// On first call, `current` is saved as the draft. Returns `None` if
    /// the history is empty or the cursor is already at the oldest entry.
    pub fn prev(&mut self, current: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        match self.cursor {
            None => {
                self.draft = current.to_string();
                self.cursor = Some(self.entries.len() - 1);
            }
            Some(0) => return Some(&self.entries[0]),
            Some(ref mut i) => *i -= 1,
        }
        Some(&self.entries[self.cursor.unwrap()])
    }

    /// Move to the next (newer) command.
    ///
    /// Returns `None` when the cursor moves past the newest entry, signalling
    /// that the caller should restore the draft (obtainable via [`draft`]).
    pub fn next(&mut self) -> Option<&str> {
        let cursor = self.cursor?;
        if cursor >= self.entries.len() - 1 {
            self.cursor = None;
            return None;
        }
        self.cursor = Some(cursor + 1);
        Some(&self.entries[self.cursor.unwrap()])
    }

    /// The draft text saved when navigation began.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Iterate all entries from oldest to newest.
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    /// Reset navigation state without changing entries.
    pub fn reset_cursor(&mut self) {
        self.cursor = None;
        self.draft.clear();
    }

    /// Load history from a plain text file (one command per line).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading history from {}", path.display()))?;
        let mut h = CommandHistory::new();
        for line in text.lines() {
            if !line.is_empty() {
                h.entries.push_back(line.to_string());
            }
        }
        // Trim to max_len in case the file is larger.
        while h.entries.len() > h.max_len {
            h.entries.pop_front();
        }
        Ok(h)
    }

    /// Save history to a plain text file (one command per line).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        let content: String = self
            .entries
            .iter()
            .map(|s| format!("{}\n", s))
            .collect();
        std::fs::write(path, content)
            .with_context(|| format!("writing history to {}", path.display()))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_deduplicates_consecutive() {
        let mut h = CommandHistory::new();
        h.push("ls".to_string());
        h.push("ls".to_string());
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn push_keeps_non_consecutive_duplicates() {
        let mut h = CommandHistory::new();
        h.push("ls".to_string());
        h.push("pwd".to_string());
        h.push("ls".to_string());
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn push_trims_to_max() {
        let mut h = CommandHistory::with_max_len(10);
        for i in 0..=10 {
            h.push(format!("cmd{}", i));
        }
        assert_eq!(h.len(), 10);
        // Oldest entry (cmd0) must have been dropped.
        assert!(!h.entries.contains(&"cmd0".to_string()));
        assert!(h.entries.contains(&"cmd10".to_string()));
    }

    #[test]
    fn push_ignores_empty_string() {
        let mut h = CommandHistory::new();
        h.push(String::new());
        assert!(h.is_empty());
    }

    #[test]
    fn prev_returns_last_command() {
        let mut h = CommandHistory::new();
        h.push("ls".to_string());
        assert_eq!(h.prev(""), Some("ls"));
    }

    #[test]
    fn prev_navigates_backward() {
        let mut h = CommandHistory::new();
        h.push("ls".to_string());
        h.push("pwd".to_string());
        assert_eq!(h.prev(""), Some("pwd"));
        assert_eq!(h.prev("pwd"), Some("ls"));
    }

    #[test]
    fn prev_stays_at_oldest() {
        let mut h = CommandHistory::new();
        h.push("only".to_string());
        h.prev("");
        // Second prev when already at oldest returns the same oldest entry.
        assert_eq!(h.prev("only"), Some("only"));
    }

    #[test]
    fn prev_on_empty_history_returns_none() {
        let mut h = CommandHistory::new();
        assert_eq!(h.prev(""), None);
    }

    #[test]
    fn next_after_prev_returns_newer() {
        let mut h = CommandHistory::new();
        h.push("a".to_string());
        h.push("b".to_string());
        h.prev(""); // cursor → "b"
        h.prev(""); // cursor → "a"
        assert_eq!(h.next(), Some("b"));
    }

    #[test]
    fn next_at_newest_returns_none_and_restores_draft() {
        let mut h = CommandHistory::new();
        h.push("ls".to_string());
        h.prev("my draft");   // saves "my draft", cursor → "ls"
        assert_eq!(h.next(), None); // past newest → None
        assert_eq!(h.draft(), "my draft");
        assert!(h.cursor.is_none());
    }

    #[test]
    fn next_when_not_navigating_returns_none() {
        let mut h = CommandHistory::new();
        h.push("ls".to_string());
        assert_eq!(h.next(), None);
    }

    #[test]
    fn push_resets_cursor() {
        let mut h = CommandHistory::new();
        h.push("ls".to_string());
        h.prev(""); // start navigating
        assert!(h.cursor.is_some());
        h.push("pwd".to_string()); // pushing resets navigation
        assert!(h.cursor.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let path = std::env::temp_dir().join("sc_history_test_roundtrip.txt");

        let mut h = CommandHistory::new();
        h.push("cmd1".to_string());
        h.push("cmd2".to_string());
        h.push("cmd3".to_string());
        h.save(&path).unwrap();

        let loaded = CommandHistory::load(&path).unwrap();
        assert_eq!(loaded.entries, h.entries);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_skips_empty_lines() {
        let path = std::env::temp_dir().join("sc_history_test_empty_lines.txt");
        std::fs::write(&path, "cmd1\n\ncmd2\n").unwrap();
        let h = CommandHistory::load(&path).unwrap();
        assert_eq!(h.len(), 2);
        let _ = std::fs::remove_file(&path);
    }
}
