use super::*;

impl App {
    /// Runs `mutate`, then records `side`'s new path in its own back/forward
    /// history if `mutate` changed it. For call sites that assign a panel's
    /// `.path` directly (bypassing `navigate_to_path`), so `Alt-Left`/`Alt-Right`
    /// still see the new directory.
    pub(super) fn record_path_change<T>(&mut self, side: Side, mutate: impl FnOnce(&mut Self) -> T) -> T {
        let before = self.panel(side).path.0.clone();
        let result = mutate(self);
        if self.panel(side).path.0 != before {
            let after = self.panel(side).path.0.clone();
            self.push_path_history_for(side, &after);
        }
        result
    }

    pub(super) fn push_path_history(&mut self, path: &str) {
        self.push_path_history_for(self.active, path);
    }

    pub(super) fn push_path_history_for(&mut self, side: Side, path: &str) {
        match side {
            Side::Left => self.panel_history_left.push(path),
            Side::Right => self.panel_history_right.push(path),
        }
    }

    pub(super) fn active_panel_history(&mut self) -> &mut crate::panel_history::PanelHistory {
        match self.active {
            Side::Left => &mut self.panel_history_left,
            Side::Right => &mut self.panel_history_right,
        }
    }

    /// Navigates the active panel to `path`, leaving any search view (a search
    /// shown in the active panel is cached into the back/forward history).
    pub(super) fn navigate_to_path(&mut self, path: &str) {
        let path = absolutize_str(path);
        if !std::path::Path::new(&path).exists() {
            self.modal = Modal::Error(format!("Path no longer exists: {path}"));
            return;
        }
        match self.active_panel().content {
            PanelContent::Dir => {}
            // Keep the results reachable through Alt-Left, like jumping out on a hit.
            PanelContent::SearchResults(_) => {
                self.end_search_companions();
                self.cache_search_to_history();
                self.active_panel_mut().content = PanelContent::Dir;
            }
            // The results live in the other panel; there's nothing to cache from here.
            PanelContent::Matches(_) => self.close_search(),
        }
        let panel = self.active_panel_mut();
        let old = std::mem::replace(&mut panel.path, crate::provider::NodePath(path.clone()));
        panel.cursor = 0;
        panel.scroll = 0;
        panel.tagged.clear();
        panel.refresh();
        panel.select_child_toward(&old.0);
        self.push_path_history(&path);
    }

    pub(super) fn navigate_to_bookmark(&mut self, path: &str) {
        let path = absolutize_str(path);
        if !std::path::Path::new(&path).exists() {
            self.bookmarks.retain(|b| b != &path);
            let _ = crate::bookmarks::save(&self.bookmarks);
        }
        self.navigate_to_path(&path);
    }

    pub(super) fn quicksearch_jump(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return;
        }
        let lc = pattern.to_lowercase();
        let found = self.active_panel().entries.iter().position(|e| {
            e.name != ".." && e.name.to_lowercase().starts_with(&lc)
        });
        if let Some(idx) = found {
            let current = self.active_panel().cursor as i32;
            let vh = self.active_vh();
            self.active_panel_mut().move_cursor(idx as i32 - current, vh);
        }
    }
}
