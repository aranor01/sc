use super::*;

impl App {
    pub(super) fn handle_ipc(&mut self, msg: IpcMessage) {
        // Every action except ShowPanels is opt-in (see startup.ipc_scripting):
        // ShowPanels is load-bearing for core cd-navigation (stateless commands
        // report their final $PWD back this way) and the Ctrl+O "return to
        // panels" signal, so it's dispatched unconditionally; everything else
        // is dropped unless the user explicitly enabled it.
        if !self.config.startup.ipc_scripting && !matches!(msg, IpcMessage::ShowPanels(_)) {
            return;
        }
        match msg {
            IpcMessage::Tag(names) => {
                let entries: Vec<String> = self.active_panel().entries.iter()
                    .filter(|e| e.name != "..")
                    .map(|e| e.name.clone())
                    .collect();
                let panel = self.active_panel_mut();
                for name in names {
                    let base = std::path::Path::new(&name)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&name)
                        .to_string();
                    if entries.contains(&base) {
                        panel.tagged.insert(base);
                    }
                }
            }
            IpcMessage::Untag(names) => {
                let panel = self.active_panel_mut();
                for name in names {
                    let base = std::path::Path::new(&name)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&name)
                        .to_string();
                    panel.tagged.remove(&base);
                }
            }
            IpcMessage::TagOnly(names) => {
                let entries: Vec<String> = self.active_panel().entries.iter()
                    .filter(|e| e.name != "..")
                    .map(|e| e.name.clone())
                    .collect();
                let panel = self.active_panel_mut();
                panel.tagged.clear();
                for name in names {
                    let base = std::path::Path::new(&name)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&name)
                        .to_string();
                    if entries.contains(&base) {
                        panel.tagged.insert(base);
                    }
                }
            }
            IpcMessage::SelectGroup(pattern) => {
                let (text, is_re) = ipc_pattern(&pattern);
                if let Ok(pat) = build_filter_pattern(text, false, true, is_re) {
                    let names: Vec<String> = self.active_panel().entries.iter()
                        .filter(|e| e.name != ".." && pat.matches(&e.name))
                        .map(|e| e.name.clone())
                        .collect();
                    let panel = self.active_panel_mut();
                    for name in names { panel.tagged.insert(name); }
                }
            }
            IpcMessage::UnselectGroup(pattern) => {
                let (text, is_re) = ipc_pattern(&pattern);
                if let Ok(pat) = build_filter_pattern(text, false, true, is_re) {
                    self.active_panel_mut().tagged.retain(|n| !pat.matches(n));
                }
            }
            IpcMessage::InjectToCommandLine(text, mode) => {
                match mode {
                    CmdlineInjectMode::Insert => {}
                    CmdlineInjectMode::Append => self.cmdline.cursor = self.cmdline.text.len(),
                    CmdlineInjectMode::Replace => self.cmdline.clear(),
                }
                self.cmdline.insert_str(&text);
                self.show_cmdline = true;
            }
            IpcMessage::Filter(pattern) => {
                if !matches!(self.active_panel().content, PanelContent::Dir) {
                    self.set_status("Filter is not available in search panels", true);
                    return;
                }
                if pattern.is_empty() {
                    self.active_panel_mut().filter = None;
                } else {
                    let (text, is_re) = ipc_pattern(&pattern);
                    if let Ok(pat) = build_filter_pattern(text, false, true, is_re) {
                        self.active_panel_mut().filter = Some(pat);
                    }
                }
                self.active_panel_mut().refresh();
            }
            IpcMessage::ToggleShell => {
                self.handle_action(Action::ToggleShell);
            }
            IpcMessage::RefreshPanel => {
                self.active_panel_mut().refresh();
            }
            IpcMessage::ShowPanels(maybe_cwd) => {
                self.show_output = false;
                if let Some(new_cwd) = maybe_cwd {
                    // Skipped for a Matches panel: it isn't a directory browser, so
                    // there's no path to navigate it to, and doing so anyway would
                    // reset its cursor/scroll.
                    if !matches!(self.active_panel().content, PanelContent::Matches(_)) {
                        let panel_cwd = self.active_panel().path.0.clone();
                        if new_cwd != panel_cwd {
                            self.navigate_to_path(&new_cwd);
                        }
                    }
                }
            }
        }
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        if self.mouse {
            execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
        } else {
            execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        }
        let _guard = TerminalGuard;

        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;

        loop {
            if self.needs_full_redraw {
                self.needs_full_redraw = false;
                terminal.clear()?;
            }
            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => {
                        self.handle_key_event(key);
                        if self.should_quit {
                            break;
                        }
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse_event(mouse);
                        if self.should_quit {
                            break;
                        }
                    }
                    Event::Resize(cols, rows) => {
                        if let ShellMode::Subshell(ref sub) = self.shell_mode {
                            sub.resize(cols, rows);
                        }
                        self.overlay.refresh();
                    }
                    Event::Paste(text) => {
                        for c in text.chars() {
                            let key = match c {
                                '\n' | '\r' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                                '\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                                _ => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                            };
                            self.handle_key_event(key);
                            if self.should_quit {
                                break;
                            }
                        }
                        if self.should_quit {
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if let Some(msg) = self.ipc.as_ref().and_then(|ipc| ipc.try_recv()) {
                self.handle_ipc(msg);
            }

            self.poll_search();
            self.sync_matches_panel();
        }

        self.save_state();
        Ok(())
    }
}

/// Parse an IPC pattern: strip the legacy `/` prefix for regex, return (text, is_regexp).
pub(super) fn ipc_pattern(pattern: &str) -> (&str, bool) {
    if let Some(rest) = pattern.strip_prefix('/') {
        (rest, true)
    } else {
        (pattern, false)
    }
}
