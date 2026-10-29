use super::*;

impl App {
    /// Every command the palette can run: built-ins from the parsed CheatSheet (only those
    /// whose config key this build knows), then the user's menu entries.
    pub(super) fn palette_entries(&self) -> Vec<PaletteEntry> {
        let bl = self.bindings_list();
        let mut out = Vec::new();
        for c in self.palette_commands.iter().filter(|c| !c.viewer) {
            let Some((_, bindings, action)) = bl.iter().find(|(n, _, _)| *n == c.config_key) else { continue };
            out.push(PaletteEntry {
                id: c.config_key.clone(),
                category: c.category.clone(),
                label: c.label.clone(),
                description: c.description.clone(),
                summary: crate::palette::short_description(&c.description).to_string(),
                hint: binding_hint(bindings),
                target: PaletteTarget::Builtin(*action),
            });
        }
        for item in &self.config.menu {
            out.push(PaletteEntry {
                id: format!("user:{}", item.label),
                category: crate::palette::USER_CATEGORY.to_string(),
                label: item.label.clone(),
                description: item.command.clone(),
                summary: String::new(),
                hint: item.keys.clone().unwrap_or_default(),
                target: PaletteTarget::Menu(item.command.clone()),
            });
        }
        out
    }

    pub(super) fn open_palette(&mut self, view: PaletteView) {
        self.completion = None;
        self.reverse_search = None;
        self.quicksearch = None;
        self.palette = Some(PaletteSession {
            list: PopupListState::new(Vec::new()),
            rows: Vec::new(),
            query: String::new(),
            view,
        });
        self.rebuild_palette();
    }

    /// Recomputes the palette list from its current view and query.
    pub(super) fn rebuild_palette(&mut self) {
        let entries = self.palette_entries();
        let Some(session) = self.palette.as_mut() else { return };
        let user_cat = crate::palette::USER_CATEGORY;
        let mut shown: Vec<(String, String, PaletteRow)> = Vec::new();
        let row_of = |e: &PaletteEntry| PaletteRow { id: e.id.clone(), target: e.target.clone() };

        if session.query.trim().is_empty() {
            match &session.view {
                PaletteView::Root => {
                    for id in &self.recent_actions {
                        if let Some(e) = entries.iter().find(|e| &e.id == id) {
                            shown.push((format!("\u{21ba} {}", e.display()), e.hint.clone(), row_of(e)));
                        }
                    }
                    let mut cats: Vec<&str> = Vec::new();
                    for e in &entries {
                        if !cats.contains(&e.category.as_str()) {
                            cats.push(&e.category);
                        }
                    }
                    for c in cats {
                        shown.push((format!("\u{25b8} {c}"), String::new(),
                            PaletteRow { id: String::new(), target: PaletteTarget::Category(c.to_string()) }));
                    }
                }
                PaletteView::Category(c) => {
                    for e in entries.iter().filter(|e| &e.category == c) {
                        shown.push((e.display(), e.hint.clone(), row_of(e)));
                    }
                }
                PaletteView::UserOnly => {
                    for e in entries.iter().filter(|e| e.category == user_cat) {
                        shown.push((e.display(), e.hint.clone(), row_of(e)));
                    }
                }
            }
        } else {
            let mut scored: Vec<(i32, &PaletteEntry)> = entries
                .iter()
                .filter(|e| match &session.view {
                    PaletteView::Root => true,
                    PaletteView::Category(c) => &e.category == c,
                    PaletteView::UserOnly => e.category == user_cat,
                })
                .filter_map(|e| {
                    let q = &session.query;
                    crate::palette::fuzzy_score(q, &e.label).map(|s| s + 100).or_else(|| {
                        let hay = format!("{} {} {}", e.category, e.label, e.description);
                        crate::palette::fuzzy_score(q, &hay)
                    }).map(|s| (s, e))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, e) in scored {
                let text = if session.view == PaletteView::Root && e.category != user_cat {
                    format!("{}: {}", e.category, e.display())
                } else {
                    e.display()
                };
                shown.push((text, e.hint.clone(), row_of(e)));
            }
        }

        if shown.is_empty() {
            shown.push(("No matching actions".to_string(), String::new(),
                PaletteRow { id: String::new(), target: PaletteTarget::Nothing }));
        }
        session.list.items = shown.iter().map(|r| r.0.clone()).collect();
        session.list.hints = shown.iter().map(|r| r.1.clone()).collect();
        session.rows = shown.into_iter().map(|r| r.2).collect();
        session.list.selected = 0;
        self.palette_popup_offset.set(0);
    }

    /// Runs the highlighted palette row (Enter / click). A category row drills into it.
    pub(super) fn palette_accept(&mut self) {
        let Some(session) = self.palette.as_ref() else { return };
        let Some(row) = session.rows.get(session.list.selected) else { return };
        let (id, target) = (row.id.clone(), row.target.clone());
        match target {
            PaletteTarget::Nothing => {}
            PaletteTarget::Category(c) => {
                if let Some(s) = self.palette.as_mut() {
                    s.view = PaletteView::Category(c);
                    s.query.clear();
                }
                self.rebuild_palette();
            }
            PaletteTarget::Builtin(action) => {
                self.palette = None;
                self.record_recent_action(&id);
                self.handle_action(action);
            }
            PaletteTarget::Menu(cmd) => {
                self.palette = None;
                self.record_recent_action(&id);
                self.execute_menu_item(cmd);
            }
        }
    }

    pub(super) fn record_recent_action(&mut self, id: &str) {
        // Opening the palette itself from the palette is noise in the recent list.
        if id != "action_palette" && id != "user_menu" {
            crate::palette::push_recent(&mut self.recent_actions, id);
        }
    }

    /// Back from a category to the Root view, with that category highlighted.
    pub(super) fn palette_leave_category(&mut self) {
        let Some(session) = self.palette.as_mut() else { return };
        let PaletteView::Category(name) = std::mem::replace(&mut session.view, PaletteView::Root) else { return };
        session.query.clear();
        self.rebuild_palette();
        if let Some(session) = self.palette.as_mut() {
            if let Some(i) = session.rows.iter().position(|r| matches!(&r.target, PaletteTarget::Category(c) if *c == name)) {
                session.list.selected = i;
            }
        }
    }

    /// Key handling while the palette is open. Always consumes the key.
    pub(super) fn handle_palette_key(&mut self, event: &KeyEvent) {
        let vh = self.palette_popup_area.get().height.saturating_sub(2) as usize;
        let Some(session) = self.palette.as_mut() else { return };
        let plain = event.modifiers == KeyModifiers::NONE;
        let in_category = matches!(session.view, PaletteView::Category(_));
        match event.code {
            KeyCode::Esc if plain => {
                if in_category {
                    self.palette_leave_category();
                } else {
                    self.palette = None;
                }
            }
            KeyCode::Enter if plain => self.palette_accept(),
            KeyCode::Backspace if plain => {
                if session.query.pop().is_some() {
                    self.rebuild_palette();
                } else if in_category {
                    self.palette_leave_category();
                }
            }
            KeyCode::Tab => {}
            _ => {
                if let PopupOutcome::InsertChar(c) = session.list.handle_key(event, vh) {
                    session.query.push(c);
                    self.rebuild_palette();
                }
            }
        }
    }
}
