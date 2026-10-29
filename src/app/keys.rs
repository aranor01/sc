use super::*;

impl App {
    pub(super) fn handle_key_event(&mut self, event: KeyEvent) {
        // Some terminals send ^H (0x08) for Backspace instead of DEL (0x7f).
        // Crossterm maps 0x08 to Ctrl+H — normalize it back to Backspace.
        let event = if event.code == KeyCode::Char('h')
            && event.modifiers == KeyModifiers::CONTROL
        {
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
        } else {
            event
        };
        let was_explicit = self.explicit_action_mode;
        self.handle_key_event_inner(event);
        // One-shot: clear ESC-triggered action mode after the first key is processed,
        // but only if there is text in the cmdline (empty cmdline keeps auto-action-mode).
        if was_explicit && !self.cmdline.is_empty() {
            self.explicit_action_mode = false;
        }
    }

    pub(super) fn handle_key_event_inner(&mut self, event: KeyEvent) {
        // Output overlay / text viewer active — block all input except scroll/dismiss keys
        if self.show_output || self.viewer.is_some() {
            if self.viewer.is_some()
                && crate::config::bindings_match_event(&self.config.keybindings.viewer.toggle_line_numbers, &event)
            {
                self.show_line_numbers = !self.show_line_numbers;
                return;
            }
            match self.overlay.handle_key(&event, &self.config.keybindings.toggle_shell) {
                OverlayOutcome::Dismissed => {
                    self.show_output = false;
                    self.viewer = None;
                    return;
                }
                OverlayOutcome::Consumed | OverlayOutcome::Passthrough => return,
            }
        }

        // Modal handling
        match &mut self.modal {
            Modal::None => {}
            Modal::Error(_) => {
                match event.code {
                    KeyCode::Enter | KeyCode::Esc => {
                        self.modal = Modal::None;
                    }
                    _ => {}
                }
                return;
            }
            Modal::Confirm(_) => {
                let outcome = if let Modal::Confirm(ref mut s) = self.modal {
                    s.handle_key(&event)
                } else { ModalOutcome::Consumed };
                match outcome {
                    ModalOutcome::Confirmed => {
                        if let Modal::Confirm(state) = std::mem::replace(&mut self.modal, Modal::None) {
                            self.execute_file_op(state);
                        }
                    }
                    ModalOutcome::Dismissed => self.modal = Modal::None,
                    _ => {}
                }
                return;
            }
            Modal::InputDialog(_) => {
                let outcome = if let Modal::InputDialog(ref mut s) = self.modal {
                    s.handle_key(&event)
                } else { ModalOutcome::Consumed };
                match outcome {
                    ModalOutcome::Confirmed => {
                        if let Modal::InputDialog(state) =
                            std::mem::replace(&mut self.modal, Modal::None)
                        {
                            self.execute_input_dialog(state);
                        }
                    }
                    ModalOutcome::Dismissed => self.modal = Modal::None,
                    _ => {}
                }
                return;
            }
            Modal::SearchDialog(_) => {
                let outcome = if let Modal::SearchDialog(ref mut s) = self.modal {
                    s.handle_key(&event)
                } else { ModalOutcome::Consumed };
                match outcome {
                    ModalOutcome::Confirmed => {
                        if let Modal::SearchDialog(state) =
                            std::mem::replace(&mut self.modal, Modal::None)
                        {
                            self.execute_search_dialog(state);
                        }
                    }
                    ModalOutcome::Dismissed => self.modal = Modal::None,
                    _ => {}
                }
                return;
            }
            Modal::PathHistoryList(_) => {
                let vh = self.path_history_popup_area.get().height.saturating_sub(2) as usize;
                let outcome = if let Modal::PathHistoryList(ref mut s) = self.modal {
                    s.handle_key(&event, vh)
                } else { PopupOutcome::Dismissed };
                match outcome {
                    PopupOutcome::Accept(path) => {
                        self.modal = Modal::None;
                        self.navigate_to_path(&path);
                    }
                    PopupOutcome::Dismissed => self.modal = Modal::None,
                    _ => {}
                }
                return;
            }
            Modal::BookmarkList(_) => {
                if event.code == KeyCode::Delete && event.modifiers == KeyModifiers::NONE {
                    // Remove by index (not by value) so a duplicate path elsewhere in the
                    // list can't cause the popup's items and self.bookmarks to desync.
                    let removed = if let Modal::BookmarkList(ref mut s) = self.modal {
                        let idx = s.selected;
                        s.remove_selected().map(|path| (idx, path))
                    } else { None };
                    if let Some((idx, path)) = removed {
                        if idx < self.bookmarks.len() {
                            self.bookmarks.remove(idx);
                        }
                        let _ = crate::bookmarks::save(&self.bookmarks);
                        self.set_status(&format!("Bookmark removed: {path}"), false);
                    }
                    if self.bookmarks.is_empty() {
                        self.modal = Modal::None;
                    }
                    return;
                }
                let vh = self.bookmark_popup_area.get().height.saturating_sub(2) as usize;
                let outcome = if let Modal::BookmarkList(ref mut s) = self.modal {
                    s.handle_key(&event, vh)
                } else { PopupOutcome::Dismissed };
                match outcome {
                    PopupOutcome::Accept(path) => {
                        self.modal = Modal::None;
                        self.navigate_to_bookmark(&path);
                    }
                    PopupOutcome::Dismissed => self.modal = Modal::None,
                    _ => {}
                }
                return;
            }
            Modal::SortPopup(_, _) => {
                let vh = self.sort_popup_area.get().height.saturating_sub(2) as usize;
                let outcome = if let Modal::SortPopup(ref mut s, _) = self.modal {
                    s.handle_key(&event, vh)
                } else { PopupOutcome::Dismissed };
                match outcome {
                    PopupOutcome::Accept(_) => {
                        if let Modal::SortPopup(state, side) =
                            std::mem::replace(&mut self.modal, Modal::None)
                        {
                            let (key, asc) = Self::sort_item_to_key(state.selected);
                            let panel = match side {
                                Side::Left => &mut self.left,
                                Side::Right => &mut self.right,
                            };
                            panel.sort_key = key;
                            panel.sort_asc = asc;
                            panel.refresh();
                        }
                    }
                    PopupOutcome::Dismissed => self.modal = Modal::None,
                    _ => {}
                }
                return;
            }
        }

        // Command palette: owns the keyboard while open.
        if self.palette.is_some() {
            self.handle_palette_key(&event);
            return;
        }

        // Completion popup: intercept keys while a candidate list is visible.
        if let Some(ref mut session) = self.completion {
            let vh = self.completion_popup_area.get().height.saturating_sub(2) as usize;
            match session.list.handle_key(&event, vh) {
                PopupOutcome::Accept(_) => { self.apply_completion(); return; }
                PopupOutcome::Dismissed => { self.completion = None; return; }
                PopupOutcome::Consumed => return,
                PopupOutcome::InsertChar(c) => {
                    self.cmdline.insert_char(c);
                    self.refresh_completion();
                    return;
                }
                PopupOutcome::Backspace => {
                    let last_was_space = self.cmdline.text.ends_with(' ');
                    self.cmdline.backspace();
                    if last_was_space || self.cmdline.text.trim().is_empty() {
                        self.completion = None;
                    } else {
                        let ws = self.cmdline.find_word_start();
                        if ws >= self.cmdline.text.len() {
                            self.completion = None;
                        } else {
                            self.refresh_completion();
                        }
                    }
                    return;
                }
                PopupOutcome::Passthrough => { self.completion = None; }
            }
        }

        // Reverse-search popup: intercept keys while active.
        if let Some(ref mut session) = self.reverse_search {
            let vh = self.rev_search_popup_area.get().height.saturating_sub(2) as usize;
            match session.list.handle_key(&event, vh) {
                PopupOutcome::Accept(entry) => {
                    self.cmdline.text = entry;
                    self.cmdline.move_end();
                    self.reverse_search = None;
                    return;
                }
                PopupOutcome::Dismissed => { self.reverse_search = None; return; }
                PopupOutcome::Consumed => return,
                PopupOutcome::InsertChar(c) => {
                    self.cmdline.insert_char(c);
                    self.update_reverse_search();
                    return;
                }
                PopupOutcome::Backspace => {
                    self.cmdline.backspace();
                    self.update_reverse_search();
                    return;
                }
                PopupOutcome::Passthrough => { self.reverse_search = None; }
            }
        }

        // Quicksearch: intercept keys while active
        if self.quicksearch.is_some() {
            match event.code {
                // Navigation keys: dismiss then execute
                KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
                | KeyCode::Home | KeyCode::End | KeyCode::Enter => {
                    self.quicksearch = None;
                    if event.code == KeyCode::Enter
                        && matches!(self.active_panel().content, PanelContent::Dir)
                        && self.active_panel().current_entry().map(|e| e.kind != NodeKind::Dir).unwrap_or(false)
                    {
                        self.open_current_entry_default_action();
                        return;
                    }
                    let am = self.action_mode();
                    let vh = self.active_vh();
                    let path_before = self.active_panel().path.0.clone();
                    match self.active_panel_mut().handle_key(&event, vh, am) {
                        PanelOutcome::Consumed => {
                            let path_after = self.active_panel().path.0.clone();
                            if path_after != path_before {
                                self.push_path_history(&path_after);
                            }
                        }
                        PanelOutcome::NavError(msg) => self.set_status(&msg, true),
                        PanelOutcome::ExecuteCommand | PanelOutcome::Passthrough => {}
                    }
                }
                KeyCode::Esc => {
                    self.quicksearch = None;
                }
                KeyCode::Backspace if event.modifiers == KeyModifiers::NONE => {
                    if let Some(ref mut qs) = self.quicksearch {
                        qs.pop();
                        let pattern = qs.clone();
                        self.quicksearch_jump(&pattern);
                    }
                }
                KeyCode::Char(c)
                    if event.modifiers == KeyModifiers::NONE
                        || event.modifiers == KeyModifiers::SHIFT =>
                {
                    if let Some(ref mut qs) = self.quicksearch {
                        qs.push(c);
                        let pattern = qs.clone();
                        self.quicksearch_jump(&pattern);
                    }
                }
                _ => { self.quicksearch = None; }
            }
            return;
        }

        // Search views: Enter activates a hit / opens the viewer; Esc interrupts a
        // still-running search in place, then closes the whole search view on a
        // second press (with a non-empty cmdline, an extra leading Esc enters
        // action mode below first).
        if event.modifiers == KeyModifiers::NONE && self.action_mode() {
            match (event.code, &self.active_panel().content) {
                (KeyCode::Enter, PanelContent::SearchResults(_)) => {
                    self.activate_search_hit();
                    return;
                }
                (KeyCode::Enter, PanelContent::Matches(_)) => {
                    self.open_match_in_viewer();
                    return;
                }
                (KeyCode::Enter, PanelContent::Dir)
                    if self.active_panel().current_entry().map(|e| e.kind != NodeKind::Dir).unwrap_or(false) =>
                {
                    self.open_current_entry_default_action();
                    return;
                }
                (KeyCode::Esc, PanelContent::SearchResults(_) | PanelContent::Matches(_)) => {
                    if self.search_still_running() {
                        self.interrupt_search();
                    } else {
                        self.close_search();
                    }
                    return;
                }
                _ => {}
            }
        }

        // ESC: toggle explicit action mode when cmdline has text (one-shot panel focus).
        if event.code == KeyCode::Esc
            && event.modifiers == KeyModifiers::NONE
            && !self.show_output
            && !self.cmdline.is_empty()
        {
            self.explicit_action_mode = !self.explicit_action_mode;
            return;
        }

        // When cmdline is active (not action mode), route printable chars and Delete
        // directly to the cmdline, preventing action bindings from intercepting them.
        if !self.action_mode() {
            match event.code {
                KeyCode::Char(c)
                    if event.modifiers == KeyModifiers::NONE
                        || event.modifiers == KeyModifiers::SHIFT =>
                {
                    self.cmdline.insert_char(c);
                    return;
                }
                KeyCode::Delete if event.modifiers == KeyModifiers::NONE => {
                    self.cmdline.delete_char();
                    return;
                }
                KeyCode::Backspace if event.modifiers == KeyModifiers::NONE => {
                    self.cmdline.backspace();
                    return;
                }
                _ => {}
            }
        }

        // Alt+B: move word left when cmdline has text; otherwise the action binding
        // (toggle_button_bar) fires from match_key below.
        if event.code == KeyCode::Char('b')
            && event.modifiers == KeyModifiers::ALT
            && !self.action_mode()
        {
            self.cmdline.move_word_left();
            return;
        }

        let pending = self.pending_chord.take();
        let bl = self.bindings_list();
        match match_key(&bl, &event, pending.as_ref()) {
            KeyMatch::Act(action) => {
                self.handle_action(action);
                return;
            }
            KeyMatch::ChordStart => {
                self.pending_chord = Some(event);
                return;
            }
            KeyMatch::None => {
                // User menu shortcuts (single-key and chord) fire from the main screen.
                let menu_match = self.config.menu.iter()
                    .find(|item| menu_item_matches_key(item, pending.as_ref(), &event))
                    .map(|item| item.command.clone());
                if let Some(cmd) = menu_match {
                    self.execute_menu_item(cmd);
                    return;
                }
                // If no chord completed, check if this key starts a user menu chord.
                if pending.is_none() && menu_item_is_chord_start(&self.config.menu, &event) {
                    self.pending_chord = Some(event);
                    return;
                }
            }
        }

        // Panel navigation keys are always intercepted first.
        let am = self.action_mode();
        let vh = self.active_vh();
        let path_before = self.active_panel().path.0.clone();
        match self.active_panel_mut().handle_key(&event, vh, am) {
            PanelOutcome::Consumed => {
                // Record destination in history when a directory change occurred
                let path_after = self.active_panel().path.0.clone();
                if path_after != path_before {
                    self.push_path_history(&path_after);
                }
                return;
            }
            PanelOutcome::ExecuteCommand => { self.execute_command(true, CmdLineState::new(), true); return; }
            PanelOutcome::NavError(msg) => { self.set_status(&msg, true); return; }
            PanelOutcome::Passthrough => {}
        }

        // Cmdline key handling
        match self.cmdline.handle_key(&event) {
            CmdlineOutcome::HistoryPrev => self.handle_action(Action::CmdlineHistoryPrev),
            CmdlineOutcome::HistoryNext => self.handle_action(Action::CmdlineHistoryNext),
            CmdlineOutcome::Consumed | CmdlineOutcome::Passthrough => {}
        }
    }
}
