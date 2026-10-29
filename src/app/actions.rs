use super::*;

impl App {
    pub(super) fn handle_action(&mut self, action: Action) {
        match action {
            Action::Exit => {
                self.should_quit = true;
            }
            Action::SwitchPanel => {
                self.active = self.active.other();
            }
            Action::ToggleLayout => {
                self.orientation = match self.orientation {
                    Orientation::Vertical => Orientation::Horizontal,
                    Orientation::Horizontal => Orientation::Vertical,
                };
            }
            Action::ToggleCmdline => {
                self.show_cmdline = !self.show_cmdline;
            }
            Action::ToggleButtonBar => {
                self.show_button_bar = !self.show_button_bar;
            }
            Action::ToggleShell => {
                match &self.shell_mode {
                    ShellMode::Subshell(_) => {
                        self.run_subshell_passthrough(false);
                    }
                    ShellMode::Stateless => {
                        if self.last_output.is_some() {
                            self.show_output = !self.show_output;
                        }
                    }
                }
            }
            Action::ToggleShellAndSyncCommandLine => {
                match &self.shell_mode {
                    ShellMode::Subshell(_) => {
                        self.run_subshell_passthrough(true);
                    }
                    ShellMode::Stateless => {
                        if self.last_output.is_some() {
                            self.show_output = !self.show_output;
                        }
                    }
                }
            }
            Action::UserMenu => {
                // Reload menu entries from config.json so edits (new/fixed commands)
                // show up without restarting sc. Always re-read rather than checking
                // mtime first: the file is tiny and F2 is a rare, human-paced event,
                // so a stat-based skip would add complexity for no real savings.
                match Config::load() {
                    Ok(fresh) => self.config.menu = fresh.menu,
                    Err(e) => self.set_status(&format!("Config reload failed: {e:#}"), true),
                }
                if self.config.menu.is_empty() {
                    self.set_status("No user menu entries configured.", true);
                    return;
                }
                self.open_palette(PaletteView::UserOnly);
            }
            Action::ActionPalette => self.open_palette(PaletteView::Root),
            Action::TagFile => {
                let vh = self.active_vh();
                self.active_panel_mut().tag_toggle(vh, true);
            }
            Action::InvertTags => {
                self.active_panel_mut().invert_tags();
            }
            Action::Copy => {
                if !self.file_ops_allowed() { return; }
                let files = self.active_panel().op_files();
                if files.is_empty() {
                    self.set_status("Nothing to copy", true);
                    return;
                }
                let dst = self.inactive_panel().path.0.clone();
                let src = self.active_panel().path.0.clone();
                if src == dst {
                    self.modal = Modal::Error("Source and destination are the same directory.".to_string());
                    return;
                }
                self.modal = Modal::Confirm(ConfirmState::new(ConfirmOp::Copy, files, Some(dst)));
            }
            Action::Move => {
                if !self.file_ops_allowed() { return; }
                let files = self.active_panel().op_files();
                if files.is_empty() {
                    self.set_status("Nothing to move", true);
                    return;
                }
                let dst = self.inactive_panel().path.0.clone();
                let src = self.active_panel().path.0.clone();
                if src == dst {
                    self.modal = Modal::Error("Source and destination are the same directory.".to_string());
                    return;
                }
                self.modal = Modal::Confirm(ConfirmState::new(ConfirmOp::Move, files, Some(dst)));
            }
            Action::Delete => {
                if !self.file_ops_source_allowed() { return; }
                let files = self.active_panel().op_files();
                if !files.is_empty() {
                    self.modal = Modal::Confirm(ConfirmState::new(ConfirmOp::Delete, files, None));
                } else {
                    self.set_status("Nothing to delete", true);
                }
            }
            Action::CmdlineInsertFilename => {
                let name = self.active_panel().current_name();
                if !name.is_empty() && name != ".." {
                    self.cmdline.insert_str(&crate::macros::shell_escape(&name));
                } else {
                    self.set_status("No file selected", true);
                }
            }
            Action::CmdlineInsertFullpath => {
                let panel = self.active_panel();
                let name = panel.current_name();
                if !name.is_empty() && name != ".." {
                    let full = format!("{}/{}", panel.path.0, name);
                    self.cmdline.insert_str(&crate::macros::shell_escape(&full));
                }
            }
            Action::CmdlineInsertTagged => {
                let panel = self.active_panel();
                let mut names: Vec<String> = panel.tagged.iter().cloned().collect();
                if names.is_empty() {
                    if let Some(e) = panel.current_entry().filter(|e| e.name != "..") {
                        names.push(e.name.clone());
                    }
                }
                names.sort();
                let s = names.iter().map(|n| crate::macros::shell_escape(n)).collect::<Vec<_>>().join(" ");
                if !s.is_empty() {
                    self.cmdline.insert_str(&s);
                }
            }
            Action::CmdlineInsertTaggedOther => {
                if !matches!(self.inactive_panel().content, PanelContent::Dir) {
                    self.set_status("The inactive panel is not a normal panel", true);
                    return;
                }
                let panel = self.inactive_panel();
                let mut names: Vec<String> = panel.tagged.iter().cloned().collect();
                if names.is_empty() {
                    if let Some(e) = panel.current_entry().filter(|e| e.name != "..") {
                        names.push(e.name.clone());
                    }
                }
                names.sort();
                let s = names.iter().map(|n| crate::macros::shell_escape(n)).collect::<Vec<_>>().join(" ");
                if !s.is_empty() {
                    self.cmdline.insert_str(&s);
                }
            }
            Action::CmdlineInsertPath => {
                let path = self.active_panel().path.0.clone();
                self.cmdline.insert_str(&crate::macros::shell_escape(&path));
            }
            Action::CmdlineInsertPathOther => {
                if !matches!(self.inactive_panel().content, PanelContent::Dir) {
                    self.set_status("The inactive panel is not a normal panel", true);
                    return;
                }
                let path = self.inactive_panel().path.0.clone();
                self.cmdline.insert_str(&crate::macros::shell_escape(&path));
            }
            Action::CmdlineHistoryPrev => {
                let current = self.cmdline.text.clone();
                if let Some(cmd) = self.history.prev(&current) {
                    let s = cmd.to_string();
                    self.cmdline.text = s;
                    self.cmdline.move_end();
                }
            }
            Action::CmdlineHistoryNext => {
                match self.history.next() {
                    Some(cmd) => {
                        let s = cmd.to_string();
                        self.cmdline.text = s;
                        self.cmdline.move_end();
                    }
                    None => {
                        let draft = self.history.draft().to_string();
                        self.cmdline.text = draft;
                        self.cmdline.move_end();
                    }
                }
            }
            Action::CmdlineComplete => {
                if !self.cmdline.text.trim().is_empty() {
                    let (candidates, warn) = self.collect_candidates();
                    if let Some(msg) = warn {
                        self.set_status(&msg, true);
                    }
                    let word_start = self.cmdline.find_word_start();
                    match candidates.len() {
                        0 => {}
                        1 => {
                            let c = candidates[0].clone();
                            self.apply_word_replacement(word_start, &c);
                        }
                        _ => {
                            self.completion = Some(CompletionSession {
                                list: PopupListState::new(candidates),
                                word_start,
                            });
                        }
                    }
                }
            }
            Action::ReverseSearch => {
                let items = history_matches(&self.history, &self.cmdline.text);
                let selected = items.len().saturating_sub(1);
                self.reverse_search = Some(ReverseSearchSession {
                    list: PopupListState { items, selected, hints: Vec::new() },
                });
            }
            Action::SyncPanels => {
                if !matches!(self.inactive_panel().content, PanelContent::Dir) {
                    self.set_status("Sync is not available while the matches panel is shown", true);
                    return;
                }
                let path = self.active_panel().path.clone();
                let inactive_side = self.active.other();
                self.record_path_change(inactive_side, |app| {
                    let inactive = app.panel_mut(inactive_side);
                    inactive.path = path;
                    inactive.cursor = 0;
                    inactive.scroll = 0;
                    inactive.tagged.clear();
                    inactive.refresh();
                });
            }
            Action::Rename => {
                if !matches!(self.active_panel().content, PanelContent::Dir) {
                    self.set_status("Rename is not available in search panels", true);
                    return;
                }
                let name = self.active_panel().current_name();
                if !name.is_empty() && name != ".." {
                    let state = InputDialogState::new(InputDialogAction::Rename, " Rename ", &name);
                    self.modal = Modal::InputDialog(state);
                }
            }
            Action::SortPanel => {
                let panel = self.active_panel();
                let selected = Self::sort_item_index(panel.sort_key, panel.sort_asc);
                let popup = PopupListState { items: Self::sort_popup_items(), selected, hints: Vec::new() };
                self.modal = Modal::SortPopup(popup, self.active);
            }
            Action::Quicksearch => {
                self.quicksearch = Some(String::new());
            }
            Action::ToggleHidden => {
                let panel = self.active_panel_mut();
                panel.show_hidden = !panel.show_hidden;
                panel.refresh();
                let msg = if panel.show_hidden { "Showing hidden files" } else { "Hiding hidden files" };
                self.set_status(msg, false);
            }
            Action::BookmarkOpen => {
                if self.bookmarks.is_empty() {
                    self.set_status("No bookmarks saved. Use C-b to add one.", true);
                    return;
                }
                let popup = PopupListState::new(self.bookmarks.clone());
                self.modal = Modal::BookmarkList(popup);
            }
            Action::Mkdir => {
                if !matches!(self.active_panel().content, PanelContent::Dir) {
                    self.set_status("Mkdir is not available in search panels", true);
                    return;
                }
                let state = InputDialogState::new(InputDialogAction::Mkdir, " Create directory ", "");
                self.modal = Modal::InputDialog(state);
            }
            Action::PathHistory => {
                if matches!(self.active_panel().content, PanelContent::Matches(_)) {
                    return;
                }
                // Opening the popup doesn't commit to navigating anywhere —
                // Esc can still dismiss it with no effect — so the search
                // view (if any) is left alone here and only closed once an
                // entry is actually picked, in the Accept handlers below.
                let history = match self.active {
                    Side::Left => &self.panel_history_left,
                    Side::Right => &self.panel_history_right,
                };
                if history.entries.is_empty() {
                    self.set_status("No path history.", true);
                    return;
                }
                let popup = PopupListState::new(
                    history.unique_entries().into_iter().map(str::to_owned).collect()
                );
                self.modal = Modal::PathHistoryList(popup);
            }
            Action::Filter => {
                if !matches!(self.active_panel().content, PanelContent::Dir) {
                    self.set_status("Filter is not available in search panels", true);
                    return;
                }
                let (prefill, opts) = match self.active_panel().filter.as_ref() {
                    Some(f) => (f.raw.clone(), CheckboxOptions {
                        files_only: f.files_only,
                        case_sensitive: f.case_sensitive,
                        is_regexp: f.is_regex,
                    }),
                    None => (String::new(), CheckboxOptions::default()),
                };
                let state = InputDialogState::new_pattern(
                    InputDialogAction::Filter, " Filter ", &prefill, opts,
                );
                self.modal = Modal::InputDialog(state);
            }
            Action::SelectGroup => {
                let state = InputDialogState::new_pattern(
                    InputDialogAction::SelectGroup, " Select group ", "", CheckboxOptions::default(),
                );
                self.modal = Modal::InputDialog(state);
            }
            Action::UnselectGroup => {
                let state = InputDialogState::new_pattern(
                    InputDialogAction::UnselectGroup, " Unselect group ", "", CheckboxOptions::default(),
                );
                self.modal = Modal::InputDialog(state);
            }
            Action::RefreshPanel => {
                if matches!(self.active_panel().content, PanelContent::SearchResults(_)) {
                    self.restart_search();
                    return;
                }
                self.active_panel_mut().refresh();
            }
            Action::Search => {
                if matches!(self.active_panel().content, PanelContent::Matches(_)) {
                    self.set_status("Search is not available from the matches panel", true);
                    return;
                }
                let mut state = SearchDialogState::new(self.active_panel().show_hidden);
                if let PanelContent::SearchResults(sr) = &self.active_panel().content {
                    let q = &sr.query;
                    state.pattern.text = q.pattern.clone();
                    state.pattern.cursor = state.pattern.text.len();
                    if let Some(c) = &q.content {
                        state.content.text = c.clone();
                        state.content.cursor = c.len();
                    }
                    if let Some(d) = q.max_depth {
                        state.depth.text = d.to_string();
                        state.depth.cursor = state.depth.text.len();
                    }
                    state.is_regexp = q.is_regex;
                    state.case_sensitive = q.case_sensitive;
                    state.content_is_regex = q.content_is_regex;
                    state.content_case_sensitive = q.content_case_sensitive;
                    state.content_whole_words = q.content_whole_words;
                    state.include_hidden = q.include_hidden;
                    state.follow_symlinks = q.follow_symlinks;
                }
                self.modal = Modal::SearchDialog(state);
            }
            Action::GoToParent => {
                if matches!(self.active_panel().content, PanelContent::Matches(_)) {
                    return;
                }
                if !matches!(self.active_panel().content, PanelContent::Dir) {
                    // Alt-Up on a search view just closes it, like Esc-Esc —
                    // it doesn't also navigate to the parent of the search root.
                    self.close_search();
                    return;
                }
                let current = self.active_panel().path.clone();
                let parent_opt = self.active_panel().provider.parent(&current);
                let Some(parent_path) = parent_opt else { return; };
                let dest = parent_path.0.clone();
                let panel = self.active_panel_mut();
                let old = std::mem::replace(&mut panel.path, parent_path);
                panel.cursor = 0;
                panel.scroll = 0;
                panel.tagged.clear();
                panel.refresh();
                panel.select_child_toward(&old.0);
                self.push_path_history(&dest);
            }
            Action::GoBack => {
                self.land_on_history_path(|h| { h.go_back() }, |h| { h.go_forward(); });
            }
            Action::GoForward => {
                self.land_on_history_path(|h| { h.go_forward() }, |h| { h.go_back(); });
            }
            Action::BookmarkAdd => {
                let path = self.active_panel().path.0.clone();
                if !self.bookmarks.contains(&path) {
                    self.bookmarks.push(path.clone());
                    let _ = crate::bookmarks::save(&self.bookmarks);
                    self.set_status(&format!("Bookmarked: {path}"), false);
                } else {
                    self.set_status("Already bookmarked", true);
                }
            }
            Action::ToggleMatchesPanel => {
                let Some(side) = self.results_side() else {
                    self.set_status("The match panel is available only for search by content results", true);
                    return;
                };
                let content_search = matches!(&self.panel(side).content, PanelContent::SearchResults(sr) if sr.content_search());
                if !content_search {
                    self.set_status("The match panel is available only for search by content results", true);
                    return;
                }
                let other = side.other();
                if matches!(self.panel(other).content, PanelContent::Matches(_)) {
                    self.matches_panel_hidden = true;
                    let panel = self.panel_mut(other);
                    panel.content = PanelContent::Dir;
                    panel.cursor = 0;
                    panel.scroll = 0;
                    panel.tagged.clear();
                    panel.refresh();
                } else {
                    self.matches_panel_hidden = false;
                    let (needle, case_sensitive, content_is_regex, content_whole_words) =
                        match &self.panel(side).content {
                            PanelContent::SearchResults(sr) => (
                                sr.query.content.clone().unwrap_or_default(),
                                sr.query.content_case_sensitive,
                                sr.query.content_is_regex,
                                sr.query.content_whole_words,
                            ),
                            _ => unreachable!(),
                        };
                    let panel = self.panel_mut(other);
                    panel.content = PanelContent::Matches(MatchesState::new(
                        needle, case_sensitive, content_is_regex, content_whole_words,
                    ));
                    panel.tagged.clear();
                    panel.cursor = 0;
                    panel.scroll = 0;
                }
            }
            Action::View => self.view_current_entry(None),
            Action::ToggleLineNumbers => {
                self.show_line_numbers = !self.show_line_numbers;
            }
        }
    }
}
