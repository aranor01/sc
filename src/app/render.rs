use super::*;

impl App {
    pub(super) fn save_state(&self) {
        let state = AppState {
            orientation: self.orientation,
            show_cmdline: self.show_cmdline,
            show_button_bar: self.show_button_bar,
            left_sort_key: self.left.sort_key,
            left_sort_asc: self.left.sort_asc,
            left_show_hidden: self.left.show_hidden,
            right_sort_key: self.right.sort_key,
            right_sort_asc: self.right.sort_asc,
            right_show_hidden: self.right.show_hidden,
            recent_actions: self.recent_actions.clone(),
            viewer: crate::state::ViewerState { show_line_numbers: self.show_line_numbers },
        };
        let _ = state.save();
        let _ = self.history.save(&crate::state::history_path());
        let _ = crate::panel_history::save(&self.panel_history_left, &self.panel_history_right);
    }

    pub(super) fn render(&mut self, frame: &mut Frame) {
        self.status.expire();

        let area = frame.area();
        let press = self.mouse_pressed;

        // Clone colorscheme so we can borrow panel states mutably without aliasing issues.
        let cs = self.config.colorscheme.clone();

        // Build the cmdline widget early so we can query needed_lines() before layout.
        let prompt;
        let mut command_line_is_active = true;
        if self.reverse_search.is_some() {
            prompt = "(reverse-i-search): ";
        } else if self.quicksearch.is_some() {
            prompt = "Search: ";
        } else {
            prompt = "$ ";
            command_line_is_active = !self.action_mode();
        }
        let cmdline_widget = CmdLineWidget { cs: &cs, prompt, active: command_line_is_active };
        let cmdline_height = if self.show_cmdline {
            cmdline_widget.needed_lines(&self.cmdline, area.width)
        } else {
            0
        };

        let show_bottom_bar = self.show_button_bar || self.status_active();
        let layout = AppLayout::compute(
            area,
            self.orientation,
            self.show_cmdline,
            show_bottom_bar,
            cmdline_height,
        );

        self.left_area.set(layout.left);
        self.right_area.set(layout.right);

        // Background
        let bg = Color::Rgb(cs.panel_bg.0, cs.panel_bg.1, cs.panel_bg.2);
        frame.render_widget(
            Block::default().style(Style::default().bg(bg)),
            area,
        );

        // Panels
        let refresh_key_hint = self.refresh_panel_key_hint();
        let left_active = self.active == Side::Left;
        let left_title = panel_title(&self.left);
        frame.render_stateful_widget(
            PanelWidget {
                cs: &cs,
                active: left_active,
                title: left_title,
                time_format: &self.config.panels.time_format,
                time_length: self.config.panels.time_length,
                refresh_key_hint: refresh_key_hint.clone(),
            },
            layout.left,
            &mut self.left,
        );
        let right_active = self.active == Side::Right;
        let right_title = panel_title(&self.right);
        frame.render_stateful_widget(
            PanelWidget {
                cs: &cs,
                active: right_active,
                title: right_title,
                time_format: &self.config.panels.time_format,
                time_length: self.config.panels.time_length,
                refresh_key_hint,
            },
            layout.right,
            &mut self.right,
        );

        // CmdLine
        if let Some(cmdline_area) = layout.cmdline {
            let buf = frame.buffer_mut();
            // When quicksearch is active, show the search pattern instead of cmdline text
            let qs_state;
            let render_state = if let Some(ref pattern) = self.quicksearch {
                qs_state = {
                    let mut s = CmdLineState::new();
                    s.text = pattern.clone();
                    s.cursor = pattern.len();
                    s
                };
                &qs_state
            } else {
                &self.cmdline
            };
            let cursor_pos = cmdline_widget.render_with_cursor(cmdline_area, buf, render_state);
            if let Some(pos) = cursor_pos {
                if matches!(self.modal, Modal::None)
                    && !self.show_output
                    && self.viewer.is_none()
                    && command_line_is_active
                {
                    frame.set_cursor_position(pos);
                }
            }
        }
        // Store InputDialog cursor to set after modal rendering
        let mut input_dialog_cursor: Option<Position> = None;

        // Popup list rendering (completion & reverse-search); shown only without modal/overlay
        if matches!(self.modal, Modal::None) && !self.show_output {
            if let Some(cmdline_area) = layout.cmdline {
                let width = cmdline_area.width as usize;

                if let Some(session) = self.completion.as_ref() {
                    if width > 0 {
                        let prompt_len = prompt.chars().count();
                        let anchor_byte = self.cmdline.find_word_start();
                        let anchor_chars = self.cmdline.text[..anchor_byte].chars().count();
                        let total_col = prompt_len + anchor_chars;
                        let anchor_x = cmdline_area.x + (total_col % width) as u16;
                        let anchor_y = cmdline_area.y + (total_col / width) as u16;
                        let (r, offset) = PopupListWidget { cs: &cs, state: &session.list, title: None, direction: PopupDirection::Above, fixed_width: None, title_left: false }
                            .render_at(area, frame.buffer_mut(), anchor_x, anchor_y, self.completion_popup_offset.get());
                        self.completion_popup_area.set(r);
                        self.completion_popup_offset.set(offset);
                    }
                } else {
                    self.completion_popup_area.set(Rect::default());
                    self.completion_popup_offset.set(0);
                }

                if let Some(session) = self.reverse_search.as_ref() {
                    let (r, offset) = PopupListWidget { cs: &cs, state: &session.list, title: None, direction: PopupDirection::Above, fixed_width: None, title_left: false }
                        .render_at(area, frame.buffer_mut(), cmdline_area.x, cmdline_area.y, self.rev_search_popup_offset.get());
                    self.rev_search_popup_area.set(r);
                    self.rev_search_popup_offset.set(offset);
                } else {
                    self.rev_search_popup_area.set(Rect::default());
                    self.rev_search_popup_offset.set(0);
                }
            }
        }

        // Command palette: centred horizontally from the second row down, with the
        // `> query` prompt as a left-aligned header.
        if let Some(session) = self.palette.as_ref().filter(|_| matches!(self.modal, Modal::None) && !self.show_output) {
            let width = PALETTE_MAX_WIDTH.min(area.width);
            // Borders (2) + title padding (2) + "> " (2) + cursor block (1) are not query room;
            // a longer query shows its tail behind a leading '…' so the cursor stays visible.
            let room = (width as usize).saturating_sub(7).max(1);
            let qlen = session.query.chars().count();
            let shown = if qlen > room {
                let tail: String = session.query.chars().skip(qlen - (room - 1)).collect();
                format!("\u{2026}{tail}")
            } else {
                session.query.clone()
            };
            let title = format!("> {shown}\u{2588}");
            let anchor_x = area.x + area.width.saturating_sub(width) / 2;
            let (r, offset) = PopupListWidget {
                cs: &cs, state: &session.list, title: Some(&title), direction: PopupDirection::Below,
                fixed_width: Some(PALETTE_MAX_WIDTH), title_left: true,
            }
            .render_at(area, frame.buffer_mut(), anchor_x, area.y, self.palette_popup_offset.get());
            self.palette_popup_area.set(r);
            self.palette_popup_offset.set(offset);
        } else {
            self.palette_popup_area.set(Rect::default());
            self.palette_popup_offset.set(0);
        }

        // Button bar / status bar
        if let Some(bb_area) = layout.button_bar {
            self.button_bar_area.set(bb_area);
            if self.status_active() {
                frame.render_widget(
                    StatusBarWidget { cs: &cs, state: &self.status },
                    bb_area,
                );
            } else {
                frame.render_widget(
                    ButtonBarWidget { cs: &cs, kb: &self.config.keybindings, menu: &self.config.menu, press },
                    bb_area,
                );
            }
        }

        // Output overlay / text viewer — drawn after cmdline/button bar so it
        // covers the full terminal area
        if let Some(viewer) = &mut self.viewer {
            self.overlay_area.set(area);
            let highlight = viewer.highlight.as_ref().map(|(n, c, r, w)| (n.as_str(), *c, *r, *w));
            let overlay = OutputOverlayWidget {
                cs: &cs,
                text: &mut viewer.buf,
                title: &viewer.title,
                highlight,
                line_numbers: self.show_line_numbers,
            };
            frame.render_stateful_widget(overlay, area, &mut self.overlay);
        } else if self.show_output {
            if let Some(text) = &mut self.last_output {
                self.overlay_area.set(area);
                let overlay = OutputOverlayWidget {
                    cs: &cs,
                    text,
                    title: " Command Output (Esc/C-o to close) ",
                    highlight: None,
                    line_numbers: false,
                };
                frame.render_stateful_widget(overlay, area, &mut self.overlay);
            }
        }

        // Modals (drawn last, on top) — capture returned hit-test areas
        let modal_areas = match &mut self.modal {
            Modal::None => ModalAreas::None,
            Modal::Confirm(state) => {
                let a = render_confirm(area, frame.buffer_mut(), &cs, state, press);
                ModalAreas::Confirm(a)
            }
            Modal::Error(msg) => {
                let msg = msg.clone();
                let a = render_error(area, frame.buffer_mut(), &cs, &msg, press);
                ModalAreas::Error(a)
            }
            Modal::InputDialog(state) => {
                let (a, cursor) = render_input_dialog(area, frame.buffer_mut(), &cs, state, press);
                ModalAreas::InputDialog(a, cursor)
            }
            Modal::SearchDialog(state) => {
                let (a, cursor) = render_search_dialog(area, frame.buffer_mut(), &cs, state, press);
                ModalAreas::SearchDialog(a, cursor)
            }
            Modal::SortPopup(state, side) => {
                let panel_area = match side {
                    Side::Left => self.left_area.get(),
                    Side::Right => self.right_area.get(),
                };
                let anchor_x = panel_area.x + 2;
                let anchor_y = panel_area.y + 1;
                let offset = self.sort_popup_offset.get();
                let (r, new_offset) = PopupListWidget { cs: &cs, state, title: Some("Sort by"), direction: PopupDirection::Below, fixed_width: None, title_left: false }
                    .render_at(area, frame.buffer_mut(), anchor_x, anchor_y, offset);
                ModalAreas::SortPopup(r, new_offset)
            }
            Modal::PathHistoryList(state) => {
                let panel_area = match self.active {
                    Side::Left => self.left_area.get(),
                    Side::Right => self.right_area.get(),
                };
                let anchor_x = panel_area.x + 2;
                let anchor_y = panel_area.y;
                let offset = self.path_history_popup_offset.get();
                let (r, new_offset) = PopupListWidget { cs: &cs, state, title: Some("Path History"), direction: PopupDirection::Below, fixed_width: None, title_left: false }
                    .render_at(area, frame.buffer_mut(), anchor_x, anchor_y, offset);
                ModalAreas::PathHistoryList(r, new_offset)
            }
            Modal::BookmarkList(state) => {
                let panel_area = match self.active {
                    Side::Left => self.left_area.get(),
                    Side::Right => self.right_area.get(),
                };
                let anchor_x = panel_area.x + 2;
                let anchor_y = panel_area.y;
                let offset = self.bookmark_popup_offset.get();
                let (r, new_offset) = PopupListWidget { cs: &cs, state, title: Some("Directory Bookmarks"), direction: PopupDirection::Below, fixed_width: None, title_left: false }
                    .render_at(area, frame.buffer_mut(), anchor_x, anchor_y, offset);
                ModalAreas::BookmarkList(r, new_offset)
            }
        };
        // Borrow of self.modal released; store areas for mouse hit-testing
        match modal_areas {
            ModalAreas::None => {}
            ModalAreas::Confirm(a) => {
                self.confirm_yes_btn.set(a.yes);
                self.confirm_no_btn.set(a.no);
            }
            ModalAreas::Error(a) => {
                self.error_ok_btn.set(a.ok);
            }
            ModalAreas::InputDialog(a, cursor) => {
                self.input_ok_btn.set(a.ok);
                self.input_cancel_btn.set(a.cancel);
                self.input_cb_files_only.set(a.cb_files_only);
                self.input_cb_case_sensitive.set(a.cb_case_sensitive);
                self.input_cb_regexp.set(a.cb_regexp);
                input_dialog_cursor = cursor;
            }
            ModalAreas::SearchDialog(a, cursor) => {
                self.input_ok_btn.set(a.ok);
                self.input_cancel_btn.set(a.cancel);
                self.search_cb_areas.set(a.checkboxes);
                self.search_input_areas.set(a.inputs);
                input_dialog_cursor = cursor;
            }
            ModalAreas::SortPopup(r, offset) => {
                self.sort_popup_area.set(r);
                self.sort_popup_offset.set(offset);
            }
            ModalAreas::PathHistoryList(r, offset) => {
                self.path_history_popup_area.set(r);
                self.path_history_popup_offset.set(offset);
            }
            ModalAreas::BookmarkList(r, offset) => {
                self.bookmark_popup_area.set(r);
                self.bookmark_popup_offset.set(offset);
            }
        }
        if let Some(pos) = input_dialog_cursor {
            frame.set_cursor_position(pos);
        }
    }
}

/// Panel border title, depending on what the panel is showing.
pub(super) fn panel_title(panel: &PanelState) -> String {
    match &panel.content {
        PanelContent::Dir => display_path(&panel.path.0),
        PanelContent::SearchResults(_) => format!("Search {}", display_path(&panel.path.0)),
        PanelContent::Matches(ms) => match &ms.file {
            Some(file) => format!("Matches: {}", file),
            None => "Matches".to_string(),
        },
    }
}
