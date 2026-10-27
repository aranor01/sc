use super::*;

impl App {
    // Called on every Left-button Up inside a modal; fires actions only when
    // `up` matches the stored Down position AND lands on a button (via Button::clicked).
    pub(super) fn handle_modal_click(&mut self, up: Position) {
        let yes_btn = self.confirm_yes_btn.get();
        let no_btn = self.confirm_no_btn.get();
        let ok_btn = self.error_ok_btn.get();
        let down = self.mouse_pressed;

        let input_ok = self.input_ok_btn.get();
        let input_cancel = self.input_cancel_btn.get();

        match &mut self.modal {
            Modal::None => {}
            Modal::Confirm(_) => {
                if yes_btn.clicked(down, up) {
                    if let Modal::Confirm(state) =
                        std::mem::replace(&mut self.modal, Modal::None)
                    {
                        self.execute_file_op(state);
                    }
                } else if no_btn.clicked(down, up) {
                    self.modal = Modal::None;
                }
            }
            Modal::Error(_) => {
                if ok_btn.clicked(down, up) {
                    self.modal = Modal::None;
                }
            }
            Modal::InputDialog(_) => {
                // Check checkbox clicks
                let cb_fo = self.input_cb_files_only.get();
                let cb_cs = self.input_cb_case_sensitive.get();
                let cb_re = self.input_cb_regexp.get();
                let clicked_cb: Option<usize> = if down == Some(up) {
                    if cb_fo.is_some_and(|r| r.contains(up)) { Some(1) }
                    else if cb_cs.is_some_and(|r| r.contains(up)) { Some(2) }
                    else if cb_re.is_some_and(|r| r.contains(up)) { Some(3) }
                    else { None }
                } else { None };

                if let Some(cb_idx) = clicked_cb {
                    if let Modal::InputDialog(ref mut s) = self.modal {
                        if let Some(ref mut cb) = s.checkboxes {
                            match cb_idx {
                                1 => cb.files_only = !cb.files_only,
                                2 => cb.case_sensitive = !cb.case_sensitive,
                                3 => cb.is_regexp = !cb.is_regexp,
                                _ => {}
                            }
                        }
                        s.focus.set(cb_idx);
                    }
                } else if input_ok.clicked(down, up) {
                    if let Modal::InputDialog(state) =
                        std::mem::replace(&mut self.modal, Modal::None)
                    {
                        self.execute_input_dialog(state);
                    }
                } else if input_cancel.clicked(down, up) {
                    self.modal = Modal::None;
                }
            }
            Modal::SearchDialog(_) => {
                let cbs = self.search_cb_areas.get();
                let inputs = self.search_input_areas.get();
                let clicked_cb = if down == Some(up) {
                    cbs.iter().position(|r| r.is_some_and(|r| r.contains(up)))
                } else {
                    None
                };
                let clicked_input = if down == Some(up) {
                    inputs.iter().position(|r| r.is_some_and(|r| r.contains(up)))
                } else {
                    None
                };
                if let Some(i) = clicked_cb {
                    if let Modal::SearchDialog(ref mut s) = self.modal {
                        s.toggle_checkbox(SEARCH_CB_FOCUS[i]);
                        s.focus.set(SEARCH_CB_FOCUS[i]);
                    }
                } else if let Some(i) = clicked_input {
                    if let Modal::SearchDialog(ref mut s) = self.modal {
                        s.focus.set(SEARCH_INPUT_FOCUS[i]);
                    }
                } else if input_ok.clicked(down, up) {
                    if let Modal::SearchDialog(state) =
                        std::mem::replace(&mut self.modal, Modal::None)
                    {
                        self.execute_search_dialog(state);
                    }
                } else if input_cancel.clicked(down, up) {
                    self.modal = Modal::None;
                }
            }
            Modal::PathHistoryList(_) => {
                let area = self.path_history_popup_area.get();
                let offset = self.path_history_popup_offset.get();
                if area.width > 0 {
                    let inner_y = area.y + 1;
                    let inner_bottom = area.y + area.height.saturating_sub(1);
                    if down == Some(up) && area.contains(up) && up.y >= inner_y && up.y < inner_bottom {
                        let idx = offset + (up.y - inner_y) as usize;
                        if let Modal::PathHistoryList(ref mut s) = self.modal {
                            if idx < s.items.len() { s.selected = idx; }
                        }
                        let path = if let Modal::PathHistoryList(ref s) = self.modal {
                            s.selected_item().map(String::from)
                        } else { None };
                        self.modal = Modal::None;
                        if let Some(p) = path {
                            self.navigate_from_path_history(&p);
                        }
                    } else if !area.contains(up) {
                        self.modal = Modal::None;
                    }
                }
            }
            Modal::BookmarkList(_) => {
                let area = self.bookmark_popup_area.get();
                let offset = self.bookmark_popup_offset.get();
                if area.width > 0 {
                    let inner_y = area.y + 1;
                    let inner_bottom = area.y + area.height.saturating_sub(1);
                    if down == Some(up) && area.contains(up) && up.y >= inner_y && up.y < inner_bottom {
                        let idx = offset + (up.y - inner_y) as usize;
                        if let Modal::BookmarkList(ref mut s) = self.modal {
                            if idx < s.items.len() { s.selected = idx; }
                        }
                        let path = if let Modal::BookmarkList(ref s) = self.modal {
                            s.selected_item().map(String::from)
                        } else { None };
                        self.modal = Modal::None;
                        if let Some(p) = path {
                            self.navigate_to_bookmark(&p);
                        }
                    } else if !area.contains(up) {
                        self.modal = Modal::None;
                    }
                }
            }
            Modal::SortPopup(_, _) => {
                let area = self.sort_popup_area.get();
                let offset = self.sort_popup_offset.get();
                if area.width > 0 {
                    let inner_y = area.y + 1;
                    let inner_bottom = area.y + area.height.saturating_sub(1);
                    if down == Some(up) && area.contains(up) && up.y >= inner_y && up.y < inner_bottom {
                        let row = (up.y - inner_y) as usize;
                        let idx = offset + row;
                        if let Modal::SortPopup(ref mut s, _) = self.modal {
                            if idx < s.items.len() { s.selected = idx; }
                        }
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
                    } else if !area.contains(up) {
                        self.modal = Modal::None;
                    }
                }
            }
        }
    }

    pub(super) fn handle_button_bar_click(&mut self, pos: Position) {
        let bb_area = self.button_bar_area.get();
        match ButtonBarWidget::button_at(&self.config.keybindings, &self.config.menu, bb_area, pos) {
            Some(BarAction::Fkey(n)) => self.handle_key_event(KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)),
            Some(BarAction::Menu(cmd)) => self.execute_menu_item(cmd),
            None => {}
        }
    }

    pub(super) fn handle_panel_down(&mut self, col: u16, row: u16, btn: MouseButton) {
        let left_area = self.left_area.get();
        let right_area = self.right_area.get();
        let pos = Position { x: col, y: row };

        let (clicked_side, clicked_area) = if left_area.contains(pos) {
            (Side::Left, left_area)
        } else if right_area.contains(pos) {
            (Side::Right, right_area)
        } else {
            return;
        };

        // Clicking anywhere in the panel (including its border) switches focus to it.
        if clicked_side != self.active {
            self.active = clicked_side;
        }

        let inner_y = clicked_area.y + 1;  // first row inside border = header row
        let entries_y = inner_y + 1;       // entries start below header
        if row < inner_y || row >= clicked_area.y + clicked_area.height - 1 {
            return;
        }
        // Header row click: sort by column
        if row == inner_y {
            if matches!(self.panel(clicked_side).content, PanelContent::Matches(_)) {
                return; // the matches panel has no sortable columns
            }
            let inner_x = clicked_area.x + 1;
            let inner_width = clicked_area.width.saturating_sub(2) as usize;
            let time_length = self.config.panels.time_length;
            let count_w = match &self.panel(clicked_side).content {
                PanelContent::SearchResults(sr) if sr.content_search() => 6,
                _ => 0,
            };
            let fixed = 1 + 1 + 1 + 8 + 1 + time_length + count_w; // same formula as panel.rs
            let name_width = if inner_width > fixed + 4 { inner_width - fixed } else { 4 };
            let rel_x = col.saturating_sub(inner_x) as usize;
            let panel = match clicked_side {
                Side::Left => &mut self.left,
                Side::Right => &mut self.right,
            };
            if rel_x >= 2 && rel_x < 2 + name_width {
                // Name column: toggle direction if already sorting by Name, else switch to Name
                if panel.sort_key == SortKey::Name {
                    panel.sort_asc = !panel.sort_asc;
                } else {
                    panel.sort_key = SortKey::Name;
                    panel.sort_asc = true;
                }
                panel.refresh();
            } else if rel_x > 2 + name_width + count_w && rel_x < 2 + name_width + count_w + 9 {
                // Size column
                if panel.sort_key == SortKey::Size {
                    panel.sort_asc = !panel.sort_asc;
                } else {
                    panel.sort_key = SortKey::Size;
                    panel.sort_asc = true;
                }
                panel.refresh();
            } else if rel_x >= 2 + name_width + count_w + 10 && rel_x < 2 + name_width + count_w + 10 + time_length {
                // Mtime column
                if panel.sort_key == SortKey::Modified {
                    panel.sort_asc = !panel.sort_asc;
                } else {
                    panel.sort_key = SortKey::Modified;
                    panel.sort_asc = true;
                }
                panel.refresh();
            }
            return;
        }
        let entry_row = (row - entries_y) as usize;
        let vh = clicked_area.height.saturating_sub(3).max(1) as usize;

        let now = Instant::now();
        let is_double = if let Some((last_time, last_col, last_row)) = self.last_click {
            now.duration_since(last_time) < Duration::from_millis(400)
                && last_col == col
                && last_row == row
        } else {
            false
        };
        self.last_click = Some((now, col, row));

        match btn {
            MouseButton::Left => {
                enum DoubleAct { None, Hit, Match, DefaultAction }
                self.panel_mut(clicked_side).move_cursor_to_row(entry_row, vh);
                // Only a double-click on a directory entry can change the panel's
                // path (via enter_dir()), so the before/after clone-and-compare
                // needed to record that in history is only done in that case —
                // ordinary single clicks skip it entirely.
                let (nav_err, act) = if is_double {
                    self.record_path_change(clicked_side, |app| {
                        let panel = app.panel_mut(clicked_side);
                        match &panel.content {
                            PanelContent::Dir => {
                                if panel.current_entry().map(|e| e.kind == NodeKind::Dir).unwrap_or(false) {
                                    (panel.enter_dir(), DoubleAct::None)
                                } else {
                                    (None, DoubleAct::DefaultAction)
                                }
                            }
                            PanelContent::SearchResults(_) => (None, DoubleAct::Hit),
                            PanelContent::Matches(_) => (None, DoubleAct::Match),
                        }
                    })
                } else {
                    (None, DoubleAct::None)
                };
                if let Some(err) = nav_err {
                    self.set_status(&err, true);
                }
                match act {
                    // clicked_side became the active panel above
                    DoubleAct::Hit => self.activate_search_hit(),
                    DoubleAct::Match => self.open_match_in_viewer(),
                    DoubleAct::DefaultAction => self.open_current_entry_default_action(),
                    DoubleAct::None => {}
                }
            }
            MouseButton::Right => {
                let panel = match clicked_side {
                    Side::Left => &mut self.left,
                    Side::Right => &mut self.right,
                };
                panel.move_cursor_to_row(entry_row, vh);
                panel.tag_toggle(vh, false);
            }
            MouseButton::Middle => {}
        }
    }

    pub(super) fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        let pos = Position { x: col, y: row };

        // Output overlay / text viewer — capture all mouse input; only scroll/scrollbar clicks are meaningful.
        if self.show_output || self.viewer.is_some() {
            let area = self.overlay_area.get();
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.overlay.scroll_by(-3);
                }
                MouseEventKind::ScrollDown => {
                    self.overlay.scroll_by(3);
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    let inner_y = area.y + 1;
                    let inner_w = area.width.saturating_sub(2);
                    let inner_h = area.height.saturating_sub(2) as usize;
                    let scrollbar_col = area.x + 1 + inner_w;
                    if col == scrollbar_col.saturating_sub(1) {
                        let total_lines = self.viewer.as_ref()
                            .map(|v| v.text.lines().count())
                            .or_else(|| self.last_output.as_deref().map(|t| t.lines().count()))
                            .unwrap_or(0);
                        let track_row = row.saturating_sub(inner_y) as usize;
                        self.overlay.scrollbar_click(track_row, inner_h, total_lines);
                    }
                }
                MouseEventKind::Up(_) => {
                    self.mouse_pressed = None;
                }
                _ => {}
            }
            return;
        }

        // Modals capture all mouse events.
        // Down: store press + visual update. Up: delegate to handle_modal_click
        // unconditionally — Button::clicked enforces the Down==Up requirement.
        if !matches!(self.modal, Modal::None) {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    match &mut self.modal {
                        Modal::SortPopup(s, _) => s.move_up(),
                        Modal::BookmarkList(s) => s.move_up(),
                        Modal::PathHistoryList(s) => s.move_up(),
                        _ => {}
                    }
                }
                MouseEventKind::ScrollDown => {
                    match &mut self.modal {
                        Modal::SortPopup(s, _) => s.move_down(),
                        Modal::BookmarkList(s) => s.move_down(),
                        Modal::PathHistoryList(s) => s.move_down(),
                        _ => {}
                    }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.mouse_pressed = Some(pos);
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.handle_modal_click(pos);
                    self.mouse_pressed = None;
                }
                _ => {}
            }
            return;
        }

        // Popup lists (completion / reverse-i-search) intercept mouse events while visible.
        // Down outside the popup dismisses it (= ESC) and swallows the click.
        {
            let completion_area = self.completion_popup_area.get();
            let rev_search_area = self.rev_search_popup_area.get();
            let palette_area = self.palette_popup_area.get();

            match mouse.kind {
                MouseEventKind::Down(_) => {
                    if self.palette.is_some() && palette_area.width > 0 {
                        if palette_area.contains(pos) {
                            let inner_y = palette_area.y + 1;
                            let inner_bottom = palette_area.y + palette_area.height.saturating_sub(1);
                            if pos.y >= inner_y && pos.y < inner_bottom {
                                let idx = self.palette_popup_offset.get() + (pos.y - inner_y) as usize;
                                if let Some(s) = self.palette.as_mut() {
                                    if idx < s.list.items.len() { s.list.selected = idx; }
                                }
                                self.mouse_pressed = Some(pos);
                            }
                        } else {
                            self.palette = None;
                        }
                        return;
                    }
                    if self.completion.is_some() && completion_area.width > 0 {
                        if completion_area.contains(pos) {
                            let inner_y = completion_area.y + 1;
                            let inner_bottom = completion_area.y + completion_area.height.saturating_sub(1);
                            if pos.y >= inner_y && pos.y < inner_bottom {
                                let row = (pos.y - inner_y) as usize;
                                let idx = self.completion_popup_offset.get() + row;
                                if let Some(s) = self.completion.as_mut() {
                                    if idx < s.list.items.len() { s.list.selected = idx; }
                                }
                                self.mouse_pressed = Some(pos);
                            }
                        } else {
                            self.completion = None;
                        }
                        return;
                    }
                    if self.reverse_search.is_some() && rev_search_area.width > 0 {
                        if rev_search_area.contains(pos) {
                            let inner_y = rev_search_area.y + 1;
                            let inner_bottom = rev_search_area.y + rev_search_area.height.saturating_sub(1);
                            if pos.y >= inner_y && pos.y < inner_bottom {
                                let row = (pos.y - inner_y) as usize;
                                let idx = self.rev_search_popup_offset.get() + row;
                                if let Some(s) = self.reverse_search.as_mut() {
                                    if idx < s.list.items.len() { s.list.selected = idx; }
                                }
                                self.mouse_pressed = Some(pos);
                            }
                        } else {
                            self.reverse_search = None;
                        }
                        return;
                    }
                }
                MouseEventKind::Up(_) => {
                    if self.palette.is_some() && palette_area.width > 0 {
                        let was_click = self.mouse_pressed == Some(pos);
                        self.mouse_pressed = None;
                        if was_click && palette_area.contains(pos) {
                            self.palette_accept();
                        }
                        return;
                    }
                    if self.completion.is_some() && completion_area.width > 0 {
                        let was_click = self.mouse_pressed == Some(pos);
                        self.mouse_pressed = None;
                        if was_click && completion_area.contains(pos) {
                            self.apply_completion();
                        }
                        return;
                    }
                    if self.reverse_search.is_some() && rev_search_area.width > 0 {
                        let was_click = self.mouse_pressed == Some(pos);
                        self.mouse_pressed = None;
                        if was_click && rev_search_area.contains(pos) {
                            if let Some(entry) = self.reverse_search.as_ref()
                                .and_then(|s| s.list.selected_item())
                                .map(String::from)
                            {
                                self.cmdline.text = entry;
                                self.cmdline.move_end();
                            }
                            self.reverse_search = None;
                        }
                        return;
                    }
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let is_up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                    if let Some(session) = self.palette.as_mut() {
                        if palette_area.width > 0 && palette_area.contains(pos) {
                            if is_up { session.list.move_up() } else { session.list.move_down() }
                            return;
                        }
                    }
                    if let Some(session) = self.completion.as_mut() {
                        if completion_area.width > 0 && completion_area.contains(pos) {
                            if is_up { session.list.move_up() } else { session.list.move_down() }
                            return;
                        }
                    }
                    if let Some(session) = self.reverse_search.as_mut() {
                        if rev_search_area.width > 0 && rev_search_area.contains(pos) {
                            if is_up { session.list.move_up() } else { session.list.move_down() }
                            return;
                        }
                    }
                }
                _ => {}
            }
        }

        match mouse.kind {
            // Button bar: Down records the press (for highlight); Up fires via button_at.
            MouseEventKind::Down(MouseButton::Left) => {
                if self.show_button_bar {
                    let bb = self.button_bar_area.get();
                    if bb.contains(pos) {
                        self.mouse_pressed = Some(pos);
                        return;
                    }
                }
                // Panel clicks fire immediately on Down.
                self.handle_panel_down(col, row, MouseButton::Left);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.mouse_pressed == Some(pos) && self.show_button_bar && !self.status_active() {
                    let bb = self.button_bar_area.get();
                    if bb.contains(pos) {
                        self.handle_button_bar_click(pos);
                    }
                }
                self.mouse_pressed = None;
            }
            // Right-click on panel fires immediately on Down.
            MouseEventKind::Down(btn) => {
                self.handle_panel_down(col, row, btn);
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let left_area = self.left_area.get();
                let right_area = self.right_area.get();
                let (panel, area) = if left_area.contains(pos) {
                    (&mut self.left, left_area)
                } else if right_area.contains(pos) {
                    (&mut self.right, right_area)
                } else {
                    return;
                };
                let vh = area.height.saturating_sub(3).max(1) as usize;
                let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) { -1 } else { 1 };
                panel.move_cursor(delta, vh);
            }
            _ => {}
        }
    }
}
