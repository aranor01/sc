use super::*;

impl App {
    /// Status-bar hint naming the configured `toggle_matches_panel` key, e.g.
    /// "(use Alt-m to hide matches)". None if the binding is unset or is a chord.
    pub(super) fn matches_panel_hint(&self) -> Option<String> {
        self.config.keybindings.toggle_matches_panel.iter().find_map(|b| match b {
            KeyBinding::Single(ke) => Some(format!("(use {} to hide matches)", format_key_spelled(ke))),
            KeyBinding::Chord(..) => None,
        })
    }

    /// Key label for the configured `refresh_panel` action, e.g. "Alt-r", used
    /// in a results panel's "(partial, ... to refresh)" footer. None if the
    /// binding is unset or is a chord.
    pub(super) fn refresh_panel_key_hint(&self) -> Option<String> {
        self.config.keybindings.refresh_panel.iter().find_map(|b| match b {
            KeyBinding::Single(ke) => Some(format_key_spelled(ke)),
            KeyBinding::Chord(..) => None,
        })
    }

    pub(super) fn results_side(&self) -> Option<Side> {
        if matches!(self.left.content, PanelContent::SearchResults(_)) {
            Some(Side::Left)
        } else if matches!(self.right.content, PanelContent::SearchResults(_)) {
            Some(Side::Right)
        } else {
            None
        }
    }

    pub(super) fn search_still_running(&self) -> bool {
        match self.results_side() {
            Some(side) => matches!(&self.panel(side).content, PanelContent::SearchResults(sr) if sr.running),
            None => false,
        }
    }

    pub(super) fn execute_search_dialog(&mut self, mut state: SearchDialogState) {
        if state.mode == SearchDialogMode::ViewerText {
            return self.execute_viewer_search(state);
        }
        let pattern_text = state.pattern.text.trim().to_string();
        let pattern = if pattern_text.is_empty() { "*".to_string() } else { pattern_text };
        if let Err(e) = build_filter_pattern(&pattern, false, state.case_sensitive, state.is_regexp) {
            state.error = Some(e);
            self.modal = Modal::SearchDialog(state);
            return;
        }
        let depth_text = state.depth.text.trim().to_string();
        let max_depth = if depth_text.is_empty() {
            None
        } else {
            match depth_text.parse::<u32>() {
                Ok(n) if n >= 1 => Some(n),
                _ => {
                    state.error = Some("Invalid max depth".to_string());
                    self.modal = Modal::SearchDialog(state);
                    return;
                }
            }
        };
        let content = if state.content.text.is_empty() {
            None
        } else {
            Some(state.content.text.clone())
        };
        if let Some(needle) = &content {
            if state.content_is_regex {
                if let Err(e) = ContentMatcher::build(
                    needle,
                    true,
                    state.content_case_sensitive,
                    state.content_whole_words,
                ) {
                    state.error = Some(e);
                    self.modal = Modal::SearchDialog(state);
                    return;
                }
            }
        }
        let query = SearchQuery {
            pattern,
            is_regex: state.is_regexp,
            case_sensitive: state.case_sensitive,
            content,
            content_is_regex: state.content_is_regex,
            content_case_sensitive: state.content_case_sensitive,
            content_whole_words: state.content_whole_words,
            max_depth,
            include_hidden: state.include_hidden,
            follow_symlinks: state.follow_symlinks,
        };
        self.start_search(query);
    }

    pub(super) fn start_search(&mut self, query: SearchQuery) {
        // A new search replaces any existing search view and drops any cached one.
        self.close_search();
        let side = self.active;
        self.panel_history_mut(side).clear_caches();
        self.panel_history_mut(side).truncate_forward();
        let root = self.active_panel().path.clone();
        let handle = match self.active_panel().provider.search(&root, query.clone()) {
            Ok(h) => h,
            Err(e) => {
                self.modal = Modal::Error(e.to_string());
                return;
            }
        };
        let content_search = query.content.is_some();
        let needle = query.content.clone().unwrap_or_default();
        let case_sensitive = query.content_case_sensitive;
        let content_is_regex = query.content_is_regex;
        let content_whole_words = query.content_whole_words;
        let panel = self.active_panel_mut();
        panel.content = PanelContent::SearchResults(SearchResultsState::new(query));
        panel.entries.clear();
        panel.tagged.clear();
        panel.cursor = 0;
        panel.scroll = 0;
        panel.error = None;
        if content_search {
            let inactive = self.inactive_panel_mut();
            inactive.content = PanelContent::Matches(MatchesState::new(
                needle, case_sensitive, content_is_regex, content_whole_words,
            ));
            inactive.tagged.clear();
            inactive.cursor = 0;
            inactive.scroll = 0;
        }
        self.search = Some(handle);
    }

    /// A-r in a results panel: run the same query again.
    pub(super) fn restart_search(&mut self) {
        let query = match &self.active_panel().content {
            PanelContent::SearchResults(sr) => sr.query.clone(),
            _ => return,
        };
        self.search = None; // cancel the current worker
        let side = self.active;
        self.panel_history_mut(side).clear_caches();
        self.panel_history_mut(side).truncate_forward();
        let root = self.active_panel().path.clone();
        match self.active_panel().provider.search(&root, query.clone()) {
            Ok(handle) => {
                let panel = self.active_panel_mut();
                panel.content = PanelContent::SearchResults(SearchResultsState::new(query));
                panel.entries.clear();
                panel.tagged.clear();
                panel.cursor = 0;
                panel.scroll = 0;
                panel.error = None;
                let inactive = self.inactive_panel_mut();
                if let PanelContent::Matches(ms) = &mut inactive.content {
                    ms.file = None;
                    ms.abs_path = None;
                    ms.matches.clear();
                    inactive.cursor = 0;
                    inactive.scroll = 0;
                }
                self.search = Some(handle);
            }
            Err(e) => self.modal = Modal::Error(e.to_string()),
        }
    }

    /// After `go_back()`/`go_forward()` return `path` for `side`, either replay
    /// the search cached at that history slot or navigate to it as an ordinary
    /// directory. `rollback` undoes the cursor move if a plain-directory path
    /// no longer exists on disk (mirrors each caller's own direction).
    pub(super) fn land_on_history_path(
        &mut self,
        commit: impl FnOnce(&mut crate::panel_history::PanelHistory) -> Option<String> ,
        rollback: impl FnOnce(&mut crate::panel_history::PanelHistory),
    ) {
        if matches!(self.active_panel().content, PanelContent::Matches(_)) {
            return;
        }
        let side = self.active;
        let Some(path) = commit(self.panel_history_mut(side)) else { return; };
        if !matches!(self.active_panel().content, PanelContent::Dir) {
            self.close_search();
        }

        if let Some(cache) = self.panel_history(side).current_cache().cloned() {
            if !std::path::Path::new(&cache.root.0).exists() {
                rollback(self.panel_history_mut(side));
                // The cache is now useless (its root is gone) — evict it so
                // this same slot doesn't keep failing this check forever.
                self.panel_history_mut(side).clear_caches();
                self.set_status(&format!("Path no longer exists: {}", cache.root.0), true);
                return;
            }
            self.restore_cached_search(side, cache);
            return;
        }
        let abs = absolutize_str(&path);
        if !std::path::Path::new(&abs).exists() {
            rollback(self.panel_history_mut(side));
            self.set_status(&format!("Path no longer exists: {abs}"), true);
            return;
        }
        let panel = self.panel_mut(side);
        let old = std::mem::replace(&mut panel.path, crate::provider::NodePath(abs));
        panel.cursor = 0;
        panel.scroll = 0;
        panel.tagged.clear();
        panel.refresh();
        panel.select_child_toward(&old.0);
    }

    /// Reconstruct a finished/interrupted search from its cache, replaying it
    /// rather than re-listing the directory — cursor/entries/matches come
    /// straight from the snapshot, and the companion Matches panel (content
    /// searches only) is rebuilt the same way `start_search` builds it live.
    pub(super) fn restore_cached_search(&mut self, side: Side, cache: crate::panel_history::CachedSearch) {
        self.matches_panel_hidden = false;
        let content_needle = cache.query.content.clone();
        let case_sensitive = cache.query.content_case_sensitive;
        let content_is_regex = cache.query.content_is_regex;
        let content_whole_words = cache.query.content_whole_words;
        let complete = cache.complete;
        let selected = cache.selected.clone();
        let root = cache.root.clone();
        let sr = SearchResultsState {
            query: cache.query,
            running: false,
            scanning: None,
            matches: cache.matches,
            complete,
        };
        let panel = self.panel_mut(side);
        panel.path = root;
        panel.entries = cache.entries;
        panel.content = PanelContent::SearchResults(sr);
        panel.tagged.clear();
        panel.scroll = 0;
        panel.error = None;
        // content is already SearchResults, so this dispatches to the
        // sort-only branch (no disk I/O) — applies the panel's current sort
        // to the restored entries instead of leaving snapshot-time order.
        // Must run before locating `selected`, since sorting reorders entries.
        panel.refresh();
        panel.cursor = selected
            .as_deref()
            .and_then(|name| panel.entries.iter().position(|e| e.name == name))
            .unwrap_or(0);
        if let Some(needle) = content_needle {
            let other = side.other();
            let panel_other = self.panel_mut(other);
            panel_other.content = PanelContent::Matches(MatchesState::new(
                needle, case_sensitive, content_is_regex, content_whole_words,
            ));
            panel_other.tagged.clear();
            panel_other.cursor = 0;
            panel_other.scroll = 0;
            self.sync_matches_panel();
        }
    }

    /// Restore any matches panel to a normal directory panel and stop the
    /// running worker. The results panel itself is left to the caller.
    pub(super) fn end_search_companions(&mut self) {
        self.search = None; // dropping the handle cancels the worker
        self.matches_panel_hidden = false;
        for side in [Side::Left, Side::Right] {
            let panel = self.panel_mut(side);
            if matches!(panel.content, PanelContent::Matches(_)) {
                panel.content = PanelContent::Dir;
                panel.cursor = 0;
                panel.scroll = 0;
                panel.tagged.clear();
                panel.refresh();
            }
        }
    }

    /// First Esc on a still-running search: cancel the worker but leave the
    /// results (and matches) panel showing what was found so far, marked
    /// `(partial, Alt-r to refresh)` — the same marker used when a search is
    /// interrupted by jumping away and back. Companion Matches panel is left
    /// untouched; a second Esc, now that nothing is running, does the full
    /// `close_search()`.
    pub(super) fn interrupt_search(&mut self) {
        self.search = None; // dropping the handle cancels the worker
        if let Some(side) = self.results_side() {
            if let PanelContent::SearchResults(sr) = &mut self.panel_mut(side).content {
                sr.stop(false);
            }
        }
    }

    /// Esc once the search is no longer running (already interrupted or
    /// complete): cancel the search and restore both panels to the
    /// directories they showed before (their paths never changed).
    pub(super) fn close_search(&mut self) {
        self.end_search_companions();
        for side in [Side::Left, Side::Right] {
            let panel = self.panel_mut(side);
            if matches!(panel.content, PanelContent::SearchResults(_)) {
                panel.content = PanelContent::Dir;
                panel.cursor = 0;
                panel.scroll = 0;
                panel.tagged.clear();
                panel.refresh();
            }
        }
    }

    pub(super) fn cache_search_to_history(&mut self) {
        let side = self.active;
        let root = self.active_panel().path.clone();
        let cache = match &self.active_panel().content {
            PanelContent::SearchResults(sr) => {
                let panel = self.active_panel();
                let selected = panel.current_entry().map(|e| e.name.clone());
                Some(Box::new(crate::panel_history::CachedSearch {
                    root: root.clone(),
                    query: sr.query.clone(),
                    entries: panel.entries.clone(),
                    matches: sr.matches.clone(),
                    selected,
                    complete: sr.complete,
                    index: 1,
                }))
            }
            _ => None,
        };
        if let Some(cache) = cache {
            // At most one cached search per side: drop whatever was cached
            // before attaching this one, so jumping again from an
            // already-restored cache can't leave two resident at once.
            self.panel_history_mut(side).clear_caches();
            self.panel_history_mut(side).set_search_cache(Some(cache));
        }
    }

    /// Enter on a search hit, mc-style: a directory hit becomes the panel's
    /// directory; a file hit shows its parent directory with the file selected.
    pub(super) fn activate_search_hit(&mut self) {
        let (rel, is_dir) = {
            let panel = self.active_panel();
            let Some(entry) = panel.current_entry() else { return };
            (entry.name.clone(), entry.kind == NodeKind::Dir)
        };
        let abs = {
            let panel = self.active_panel();
            panel.provider.join(&panel.path, &rel).0
        };
        self.end_search_companions();
        self.cache_search_to_history();
        let panel = self.active_panel_mut();
        panel.content = PanelContent::Dir;
        panel.tagged.clear();
        panel.cursor = 0;
        panel.scroll = 0;
        if is_dir {
            panel.path = NodePath(abs.clone());
            panel.refresh();
            self.push_path_history(&abs);
        } else {
            let p = Path::new(&abs);
            let parent = p
                .parent()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/".to_string());
            let new_path = NodePath(parent.clone());
            let is_in_root = new_path == panel.path;
            let file = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            panel.path = new_path;
            panel.refresh();
            if let Some(idx) = panel.entries.iter().position(|e| e.name == file) {
                panel.cursor = idx;
            }
            let vh = self.active_vh().max(1);
            let panel = self.active_panel_mut();
            if panel.cursor >= panel.scroll + vh {
                panel.scroll = panel.cursor + 1 - vh;
            }
            if is_in_root {
                self.active_panel_history().push_same_path_on_search_cache();
            } else {
                self.push_path_history(&parent);
            }
        }
    }

    /// Drain pending search events once per event-loop tick.
    pub(super) fn poll_search(&mut self) {
        if self.search.is_none() {
            return;
        }
        let Some(side) = self.results_side() else {
            self.search = None;
            return;
        };
        let mut hits = Vec::new();
        let mut scanning = None;
        let mut done = None;
        if let Some(handle) = self.search.as_mut() {
            while let Some(event) = handle.try_next() {
                match event {
                    SearchEvent::Hit(hit) => hits.push(hit),
                    SearchEvent::Progress { scanning: dir, .. } => scanning = Some(dir),
                    SearchEvent::Done { errors } => {
                        done = Some(errors);
                        break;
                    }
                }
            }
        }
        let panel = self.panel_mut(side);
        let root = PathBuf::from(&panel.path.0);
        for hit in hits {
            let rel = Path::new(&hit.path.0)
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| hit.path.0.clone());
            let (kind, size, modified) = match std::fs::metadata(&hit.path.0) {
                Ok(m) => (
                    if m.is_dir() { NodeKind::Dir } else { NodeKind::File },
                    m.len(),
                    m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                ),
                Err(_) => (NodeKind::File, 0, std::time::SystemTime::UNIX_EPOCH),
            };
            if let PanelContent::SearchResults(sr) = &mut panel.content {
                sr.matches.insert(rel.clone(), hit.matches);
            }
            panel.entries.push(NodeEntry {
                name: rel,
                kind,
                size,
                modified,
                permissions: String::new(),
            });
        }
        if let PanelContent::SearchResults(sr) = &mut panel.content {
            if let Some(dir) = scanning {
                sr.scanning = Some(display_path(&dir.0));
            }
            if done.is_some() {
                sr.stop(true);
            }
        }
        if let Some(errors) = done {
            self.search = None;
            self.panel_mut(side).refresh(); // apply the panel's sort order
            if !errors.is_empty() {
                self.set_status(
                    &format!("Search: {} location(s) could not be read", errors.len()),
                    true,
                );
            }
        }
    }

    /// Keep the matches panel showing the matches of the file selected in the
    /// results panel.
    pub(super) fn sync_matches_panel(&mut self) {
        let Some(side) = self.results_side() else { return };
        let other = side.other();
        if !matches!(self.panel(other).content, PanelContent::Matches(_)) {
            return;
        }
        let (sel_name, sel_matches, sel_abs) = {
            let rp = self.panel(side);
            match rp.current_entry() {
                Some(entry) => {
                    let name = entry.name.clone();
                    let matches = match &rp.content {
                        PanelContent::SearchResults(sr) => {
                            sr.matches.get(&name).cloned().unwrap_or_default()
                        }
                        _ => Vec::new(),
                    };
                    let abs = rp.provider.join(&rp.path, &name).0;
                    (Some(name), matches, Some(abs))
                }
                None => (None, Vec::new(), None),
            }
        };
        let panel = self.panel_mut(other);
        let mut changed = false;
        if let PanelContent::Matches(ms) = &mut panel.content {
            if ms.file != sel_name {
                ms.file = sel_name;
                ms.abs_path = sel_abs;
                ms.matches = sel_matches;
                changed = true;
            }
        }
        if changed {
            panel.cursor = 0;
            panel.scroll = 0;
        }
    }
}
