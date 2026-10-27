use super::*;

impl App {
    pub(super) fn reopen_dialog_with_error(&mut self, mut state: InputDialogState, err: String) {
        state.error = Some(err);
        self.modal = Modal::InputDialog(state);
    }

    pub(super) fn execute_input_dialog(&mut self, state: InputDialogState) {
        let new_text = state.input.text.trim().to_string();
        match state.action {
            InputDialogAction::Rename => {
                if new_text.is_empty() { return; }
                let current = self.active_panel().current_name();
                if current.is_empty() || current == ".." { return; }
                let result = {
                    let panel = self.active_panel();
                    let src = panel.provider.join(&panel.path, &current);
                    panel.provider.rename(&src, &new_text)
                };
                match result {
                    Ok(()) => {
                        self.active_panel_mut().refresh();
                        let idx = self.active_panel().entries.iter()
                            .position(|e| e.name == new_text)
                            .unwrap_or(0);
                        let vh = self.active_vh();
                        let panel = self.active_panel_mut();
                        panel.cursor = idx.min(panel.entries.len().saturating_sub(1));
                        let scroll = panel.scroll;
                        if panel.cursor < scroll {
                            panel.scroll = panel.cursor;
                        } else if panel.cursor >= scroll + vh.max(1) {
                            panel.scroll = panel.cursor + 1 - vh.max(1);
                        }
                    }
                    Err(e) => {
                        self.modal = Modal::Error(e.to_string());
                    }
                }
            }
            InputDialogAction::Mkdir => {
                if new_text.is_empty() { return; }
                let result = {
                    let panel = self.active_panel();
                    panel.provider.mkdir(&panel.path, &new_text)
                };
                match result {
                    Ok(()) => {
                        self.active_panel_mut().refresh();
                        let idx = self.active_panel().entries.iter()
                            .position(|e| e.name == new_text)
                            .unwrap_or(0);
                        let vh = self.active_vh();
                        let panel = self.active_panel_mut();
                        panel.cursor = idx.min(panel.entries.len().saturating_sub(1));
                        let scroll = panel.scroll;
                        if panel.cursor < scroll {
                            panel.scroll = panel.cursor;
                        } else if panel.cursor >= scroll + vh.max(1) {
                            panel.scroll = panel.cursor + 1 - vh.max(1);
                        }
                    }
                    Err(e) => {
                        self.modal = Modal::Error(e.to_string());
                    }
                }
            }
            InputDialogAction::Filter => {
                let opts = state.checkboxes.as_ref().cloned().unwrap_or_default();
                if new_text.is_empty() {
                    self.active_panel_mut().filter = None;
                    self.active_panel_mut().refresh();
                } else {
                    match build_filter_pattern(&new_text, opts.files_only, opts.case_sensitive, opts.is_regexp) {
                        Ok(pat) => {
                            self.active_panel_mut().filter = Some(pat);
                            self.active_panel_mut().refresh();
                        }
                        Err(e) => {
                            self.reopen_dialog_with_error(state, e);
                        }
                    }
                }
            }
            InputDialogAction::SelectGroup => {
                if new_text.is_empty() { return; }
                let opts = state.checkboxes.as_ref().cloned().unwrap_or_default();
                match build_filter_pattern(&new_text, opts.files_only, opts.case_sensitive, opts.is_regexp) {
                    Ok(pat) => {
                        let names: Vec<String> = self.active_panel().entries.iter()
                            .filter(|e| {
                                e.name != ".."
                                    && !(opts.files_only && e.kind == NodeKind::Dir)
                                    && pat.matches(&e.name)
                            })
                            .map(|e| e.name.clone())
                            .collect();
                        let panel = self.active_panel_mut();
                        for name in names { panel.tagged.insert(name); }
                    }
                    Err(e) => {
                        self.reopen_dialog_with_error(state, e);
                    }
                }
            }
            InputDialogAction::UnselectGroup => {
                if new_text.is_empty() { return; }
                let opts = state.checkboxes.as_ref().cloned().unwrap_or_default();
                // Collect directory names before the mutable borrow
                let dir_names: std::collections::HashSet<String> = if opts.files_only {
                    self.active_panel().entries.iter()
                        .filter(|e| e.kind == NodeKind::Dir)
                        .map(|e| e.name.clone())
                        .collect()
                } else {
                    std::collections::HashSet::new()
                };
                match build_filter_pattern(&new_text, opts.files_only, opts.case_sensitive, opts.is_regexp) {
                    Ok(pat) => {
                        let panel = self.active_panel_mut();
                        panel.tagged.retain(|n| {
                            // If files_only and this is a directory, keep it tagged
                            if pat.files_only && dir_names.contains(n) { return true; }
                            !pat.matches(n)
                        });
                    }
                    Err(e) => {
                        self.reopen_dialog_with_error(state, e);
                    }
                }
            }
        }
    }

    // ── File search ───────────────────────────────────────────────────────────

    /// F5/F6/F8 all need the active panel to hold real, taggable entries — the
    /// matches panel doesn't.
    pub(super) fn file_ops_source_allowed(&mut self) -> bool {
        if matches!(self.active_panel().content, PanelContent::Matches(_)) {
            self.set_status("No file operations in the matches panel", true);
            return false;
        }
        true
    }

    /// F5/F6 additionally need the inactive panel to be a normal directory, since
    /// that's the copy/move destination. F8 (delete) has no destination, so it only
    /// needs `file_ops_source_allowed`.
    pub(super) fn file_ops_allowed(&mut self) -> bool {
        if !self.file_ops_source_allowed() {
            return false;
        }
        if !matches!(self.inactive_panel().content, PanelContent::Dir) {
            let mut msg = "File operations need a normal panel as destination".to_string();
            if matches!(self.inactive_panel().content, PanelContent::Matches(_)) {
                if let Some(hint) = self.matches_panel_hint() {
                    msg.push(' ');
                    msg.push_str(&hint);
                }
            }
            self.set_status(&msg, true);
            return false;
        }
        true
    }

    pub(super) fn execute_file_op(&mut self, state: ConfirmState) {
        let src_path = self.active_panel().path.0.clone();
        let dst_path = self.inactive_panel().path.0.clone();
        let files = state.files.clone();

        let prov = FilesystemProvider;
        let mut errors: Vec<String> = Vec::new();

        match state.op {
            ConfirmOp::Copy => {
                let dst = crate::provider::NodePath(dst_path);
                for name in &files {
                    let src = prov.join(&crate::provider::NodePath(src_path.clone()), name);
                    if let Err(e) = prov.copy(&src, &dst) {
                        errors.push(format!("{}: {}", name, e));
                    }
                }
                if errors.is_empty() {
                    self.active_panel_mut().tagged.clear();
                }
            }
            ConfirmOp::Move => {
                let dst = crate::provider::NodePath(dst_path);
                for name in &files {
                    let src = prov.join(&crate::provider::NodePath(src_path.clone()), name);
                    if let Err(e) = prov.move_entry(&src, &dst) {
                        errors.push(format!("{}: {}", name, e));
                    }
                }
                if errors.is_empty() {
                    self.active_panel_mut().tagged.clear();
                }
            }
            ConfirmOp::Delete => {
                for name in &files {
                    let target = prov.join(&crate::provider::NodePath(src_path.clone()), name);
                    if let Err(e) = prov.delete(&target) {
                        errors.push(format!("{}: {}", name, e));
                    }
                }
                if errors.is_empty() {
                    self.active_panel_mut().tagged.clear();
                    // A results panel doesn't re-list from disk: drop the
                    // deleted hits from the shown entries ourselves.
                    let panel = self.active_panel_mut();
                    if let PanelContent::SearchResults(sr) = &mut panel.content {
                        for name in &files {
                            sr.matches.remove(name);
                        }
                        panel.entries.retain(|e| !files.contains(&e.name));
                        // If this view came from a cached search (Alt-Left),
                        // the cache is a separate copy — without this, the
                        // deleted entries would reappear next time it's replayed.
                        let side = self.active;
                        if let Some(cache) = self.panel_history_mut(side).current_cache_mut() {
                            for name in &files {
                                cache.matches.remove(name);
                            }
                            cache.entries.retain(|e| !files.contains(&e.name));
                        }
                    }
                }
            }
        }

        self.left.refresh();
        self.right.refresh();
        // Deleting a hit may have shifted the results panel's selection (or
        // emptied it); keep the companion matches panel, if any, in step
        // rather than waiting for the next tick's sync_matches_panel() call.
        self.sync_matches_panel();

        if !errors.is_empty() {
            self.modal = Modal::Error(errors.join("\n"));
        } else {
            self.modal = Modal::None;
        }
    }
}
