use super::*;

impl App {
    /// Call the completion script with the current cmdline text.
    /// Returns `(candidates, warning)` where `warning` is set when the script is
    /// missing or exits non-zero with no output.
    pub(super) fn collect_candidates(&self) -> (Vec<String>, Option<String>) {
        if self.cmdline.text.trim().is_empty() {
            return (vec![], None);
        }
        let Some(script) = find_complete_script() else {
            return (vec![], Some("Completion script not found".to_string()));
        };
        match std::process::Command::new("bash")
            .arg(&script)
            .arg(self.cmdline.get_text_before_cursor())
            .current_dir(&self.active_panel().path.0)
            .output()
        {
            Ok(out) => {
                let candidates: Vec<String> = std::str::from_utf8(&out.stdout)
                    .unwrap_or("")
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                if !out.status.success() && candidates.is_empty() {
                    (vec![], Some("Completion script failed".to_string()))
                } else {
                    (candidates, None)
                }
            }
            Err(_) => (vec![], Some("Completion script not found".to_string())),
        }
    }

    /// Replace the last word in the cmdline with `candidate`.
    pub(super) fn apply_word_replacement(&mut self, word_start: usize, candidate: &str) {
        // Find the boundary where the target word ends (the next space after word_start)
        let rest_start = self.cmdline.text[word_start..]
            .find(' ')
            .map(|relative_idx| word_start + relative_idx)
            .unwrap_or(self.cmdline.text.len());

        // Preserve the suffix after the replaced word
        let mut rest_of_text = self.cmdline.text[rest_start..].to_string();

        // Determine candidate space characteristics
        let candidate_ends_with_space = candidate.ends_with(' ') && !candidate.ends_with("\\ ");

        // Handle space duplication: if candidate ends with a space and rest_of_text 
        // starts with a space, strip the leading space from rest_of_text.
        if candidate_ends_with_space && rest_of_text.starts_with(' ') {
            rest_of_text.remove(0);
        }

        // Truncate to the word start
        self.cmdline.text.truncate(word_start);
        self.cmdline.cursor = word_start;

        // Insert the replacement candidate
        self.cmdline.insert_str(candidate);

        // Add a trailing space only if candidate doesn't end with a space AND rest_of_text doesn't start with one
        let candidate_needs_space = !candidate_ends_with_space;
        let space_already_follows = rest_of_text.starts_with(' ');

        if candidate_needs_space && !space_already_follows {
            self.cmdline.insert_str(" ");
        }

        // Save cursor: It is now right after the replaced word (and its trailing space)
        let target_cursor = self.cmdline.cursor;

        // Append the preserved suffix
        self.cmdline.insert_str(&rest_of_text);

        // Restore cursor: Put it back directly after the replacement
        self.cmdline.cursor = target_cursor;
    }

    /// Apply the currently selected popup candidate and close the popup.
    pub(super) fn apply_completion(&mut self) {
        if let Some(session) = self.completion.take() {
            if let Some(candidate) = session.list.items.get(session.list.selected).cloned() {
                self.apply_word_replacement(session.word_start, &candidate);
            }
        }
    }

    /// Re-run completion after a cmdline edit while the popup is open.
    /// Keeps the popup open (updating candidates) or closes it as needed.
    pub(super) fn refresh_completion(&mut self) {
        if self.cmdline.text.trim().is_empty() {
            self.completion = None;
            return;
        }
        let (candidates, warn) = self.collect_candidates();
        if let Some(msg) = warn {
            self.set_status(&msg, true);
            self.completion = None;
            return;
        }
        let word_start = self.cmdline.find_word_start();
        match candidates.len() {
            0 => {
                self.completion = None;
            }
            1 => {
                let candidate = candidates[0].clone();
                self.apply_word_replacement(word_start, &candidate);
                self.completion = None;
            }
            _ => {
                if let Some(session) = &mut self.completion {
                    session.list.items = candidates;
                    session.word_start = word_start;
                    session.list.selected = 0;
                }
            }
        }
    }

    /// Re-filter history after a cmdline edit during reverse-search.
    pub(super) fn update_reverse_search(&mut self) {
        let Some(session) = &mut self.reverse_search else { return };
        let new_items = history_matches(&self.history, &self.cmdline.text);
        session.list.selected = new_items.len().saturating_sub(1);
        session.list.items = new_items;
        self.rev_search_popup_offset.set(0);
    }

    // ── Command palette ───────────────────────────────────────────────────────
}

pub(super) fn find_complete_script() -> Option<PathBuf> {
    let dir = crate::config::find_scripts_dir()?;
    for name in &["sc-complete", "complete.sh"] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// All history entries that contain `filter` as a substring, oldest first.
/// If `filter` is empty, all entries are returned.
pub(super) fn history_matches(history: &crate::history::CommandHistory, filter: &str) -> Vec<String> {
    if filter.is_empty() {
        history.entries().map(String::from).collect()
    } else {
        history.entries().filter(|s| s.contains(filter)).map(String::from).collect()
    }
}

/// Byte offset of the start of the last word in `text`.
/// Returns `text.len()` when the text ends with a space (empty current word).
pub(super) fn last_word_start(text: &str) -> usize {
    if text.ends_with(' ') {
        text.len()
    } else {
        text.rfind(' ').map(|i| i + 1).unwrap_or(0)
    }
}
