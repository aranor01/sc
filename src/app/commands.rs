use super::*;

impl App {
    pub(super) fn execute_menu_item(&mut self, cmd_template: String) {
        if !matches!(self.inactive_panel().content, PanelContent::Dir)
            && template_references_inactive(&cmd_template)
        {
            self.set_status("Command references the inactive panel, which is not a normal panel now", true);
            return;
        }
        let prior_cmdline = self.cmdline.clone();
        let result = self.expand_menu_command(&cmd_template);
        if result.untag_active {
            self.active_panel_mut().tagged.clear();
        }
        if result.untag_inactive {
            self.inactive_panel_mut().tagged.clear();
        }
        self.cmdline.text = result.text;
        self.cmdline.move_end();
        self.execute_command(false, prior_cmdline, false);
    }

    /// Enter/double-click on a file with an empty command line: resolves
    /// `panels.default_action_executable`/`default_action_text` (falling back to
    /// `panels.default_action` when the specific one is empty) and runs it. `:view`
    /// opens the internal viewer; anything else is a shell command template expanded
    /// with a restricted macro context (see `expand_default_action_command`).
    pub(super) fn open_current_entry_default_action(&mut self) {
        let panel = self.active_panel();
        let Some(entry) = panel.current_entry() else { return };
        if entry.kind == NodeKind::Dir {
            return;
        }
        let executable = crate::provider::is_executable(&entry.permissions);
        // let cfg = &self.config.panels;

        let mut template: &str = if executable && !self.config.panels.default_action_executable.is_empty() {
            &self.config.panels.default_action_executable
        } else {
            let text_action = &self.config.panels.default_action_text;
            if text_action.is_empty() {
                ""
            } else if text_action == ":view" {
                if text_action == &self.config.panels.default_action {
                    // this silly configuration (both :view) is not so unlikely because one is the default value
                    // let the catchall branch handle it, so the binary check is not performed
                    text_action
                } else {
                    let mut noop_because_it_is_binary:Option<&mut bool> = Some(&mut false);
                    self.view_current_entry(noop_because_it_is_binary.as_deref_mut());
                    if *noop_because_it_is_binary.unwrap() {
                        ""
                    } else {
                        return;
                    }
                }
            } else if self.is_current_entry_text_file() {
                text_action
            } else {
                ""
            }
        };

        if template.is_empty() {
            template = &self.config.panels.default_action;
        }

        if template == ":view" {
            self.view_current_entry(None);
        } else if !template.is_empty() {
            self.execute_default_action(template.to_string());
        }
    }

    /// Runs a `panels.default_action*` command template against the active panel's
    /// current entry, with only `%f`/`%x`/`%b`/`%d` meaningful (see
    /// `expand_default_action_command`).
    pub(super) fn execute_default_action(&mut self, cmd_template: String) {
        let prior_cmdline = self.cmdline.clone();
        let result = self.expand_default_action_command(&cmd_template);
        self.cmdline.text = result.text;
        self.cmdline.move_end();
        self.execute_command(false, prior_cmdline, false);
    }

    pub(super) fn execute_command(&mut self, push_to_history: bool, restore_to: CmdLineState, echo: bool) {
        let cmd = self.cmdline.text.clone();
        if cmd.is_empty() {
            return;
        }
        if push_to_history {
            self.history.push(cmd.clone());
            let _ = self.history.save(&crate::state::history_path());
        }
        self.cmdline = restore_to;

        let cwd = self.active_panel_shell_dir();

        // Tear down TUI so any spawned process gets a real terminal.
        if self.mouse {
            let _ = crossterm::execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen, DisableMouseCapture, Show);
        } else {
            let _ = crossterm::execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen, Show);
        }
        let _ = disable_raw_mode();

        // Start on a new line — the scrollback cursor may be right after bash's
        // last prompt with no trailing newline.
        let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\r\n");

        // In subshell mode: if the persistent bash's cwd differs from the panel's,
        // show the cd visually and sync the bash now (before the command output),
        // so Ctrl+O later finds it already aligned and the order on screen is correct.
        if let ShellMode::Subshell(ref sub) = self.shell_mode {
            let shell_cwd = std::fs::read_link(format!("/proc/{}/cwd", sub.child_pid))
                .ok()
                .map(|p| p.to_string_lossy().to_string());
            if shell_cwd.as_deref() != Some(cwd.as_str()) {
                if echo {
                    let _ = std::io::Write::write_all(
                        &mut std::io::stdout(),
                        format!("[sc]$ cd {}\r\n", shell_escape_path(&cwd)).as_bytes(),
                    );
                }
                let _ = sub.send_line(&format!(" cd {}", shell_escape_path(&cwd)));
            }
        }

        // Echo the command so the user sees what ran and in what order
        // (suppressed for menu-item/custom-command execution).
        if echo {
            let _ = std::io::Write::write_all(
                &mut std::io::stdout(),
                format!("[sc]$ {}\r\n", cmd).as_bytes(),
            );
        }

        // Always run cmdline commands in a fresh PTY. This gives clean output
        // with no readline echo artifacts regardless of shell mode. The persistent
        // subshell is reserved for Ctrl+O interactive passthrough only.
        // Append sc-action so the shell reports its final $PWD back via IPC,
        // enabling transparent cd navigation in the panel.
        let binding = std::env::current_exe().unwrap().parent().unwrap().to_owned().join("sc-action");
        let sc_action_path = binding.to_str().unwrap_or("sc-action");
        let wrapped = format!(r#"{cmd}; {sc_action_path} "$SC_TOKEN" ShowPanels "$PWD""#);
        let raw = crate::subshell::run_with_pty_capture(&wrapped, &cwd);
        let text = normalize_pty_output(&raw);
        self.last_output = if text.trim().is_empty() { None } else { Some(TextBuffer::from_string(text)) };
        self.overlay.reset_scroll();

        let _ = enable_raw_mode();
        if self.mouse {
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste);
        } else {
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen, EnableBracketedPaste);
        }

        // Drain the IPC socket: picks up the ShowPanels message sent by sc-action
        // above, and also any Tag/Filter/etc. messages the command may have sent.
        let pending: Vec<_> = self.ipc.iter().flat_map(|ipc| {
            std::iter::from_fn(|| ipc.try_recv())
        }).collect();
        for msg in pending {
            self.handle_ipc(msg);
        }

        // In subshell mode, add the command to bash's in-memory history so the
        // user can recall it with Up arrow during Ctrl+O sessions (skipped for
        // menu commands, same as sc's own history above). Either way, mark that
        // a fresh prompt is needed: the cwd-sync block above may have written to
        // the subshell too, and its echo/prompt accumulates unread in the PTY
        // master buffer until the next passthrough starts and drains it.
        if let ShellMode::Subshell(ref sub) = self.shell_mode {
            if push_to_history {
                let escaped = shell_escape_path(&cmd);
                let _ = sub.send_line(&format!("history -s {escaped}"));
            }
            self.subshell_prompt_needed = true;
        }

        self.show_output = false;
        self.needs_full_redraw = true;
        self.left.refresh();
        self.right.refresh();
    }

    pub(super) fn expand_menu_command(&self, template: &str) -> crate::macros::ExpandResult {
        let active = self.active_panel();
        let inactive = self.inactive_panel();
        let active_ctx = PanelContext {
            current_file: active.current_name(),
            dir: active.path.0.clone(),
            tagged: active.tagged.iter().cloned().collect(),
        };
        let inactive_ctx = PanelContext {
            current_file: inactive.current_name(),
            dir: inactive.path.0.clone(),
            tagged: inactive.tagged.iter().cloned().collect(),
        };
        let ctx = MacroContext { active: active_ctx, inactive: inactive_ctx };
        crate::macros::expand(template, &ctx)
    }

    /// Like `expand_menu_command`, but leaves `inactive` and `active.tagged` at their
    /// defaults so `%F`/`%D`/`%t`/`%T`/`%u`/`%U`/`%S` degrade to empty and `%s` falls
    /// back to `%f` — a `panels.default_action*` command acts on a single entry, with
    /// no tagged-files or inactive-panel context to draw on.
    pub(super) fn expand_default_action_command(&self, template: &str) -> crate::macros::ExpandResult {
        let active = self.active_panel();
        let active_ctx = PanelContext {
            current_file: active.current_name(),
            dir: active.path.0.clone(),
            tagged: Vec::new(),
        };
        let ctx = MacroContext { active: active_ctx, inactive: PanelContext::default() };
        crate::macros::expand(template, &ctx)
    }

    pub(super) fn run_subshell_passthrough(&mut self, sync_cmdline: bool) {
        // Leave alternate screen so the subshell renders in the normal terminal.
        // Deliberately do NOT send DisableBracketedPaste here: the persistent subshell's
        // own readline owns bracketed-paste state across the whole passthrough session
        // (it enables it per readline() call and disables it when that call finishes).
        // If Ctrl+O interrupts bash mid-line and we forced it off here, the terminal
        // would go out of sync with what readline still believes it already turned on
        // for the in-progress line — pastes into the resumed subshell would arrive
        // unbracketed until the user finishes that line. Leaving the mode alone keeps
        // the terminal state consistent with whatever bash itself last set. We still
        // force it back on with EnableBracketedPaste below once sc regains the real
        // terminal, so sc's own TUI never depends on what bash left behind.
        if self.mouse {
            let _ = crossterm::execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture, Show);
        } else {
            let _ = crossterm::execute!(stdout(), LeaveAlternateScreen, Show);
        }
        let _ = disable_raw_mode();

        let ipc_fd = self.ipc.as_ref().map(|s| s.raw_fd());
        let cmdline_text = if sync_cmdline { self.cmdline.text.clone() } else { String::new() };
        let mut pending_ipc: Vec<String> = Vec::new();
        if let ShellMode::Subshell(ref sub) = self.shell_mode {
            let cwd = self.active_panel_shell_dir();
            // Sync PTY window size so fullscreen programs (vim, mc) get the real dimensions.
            // Do this before draining: resize() sends SIGWINCH, which makes readline
            // redraw the shell's already-idle prompt, and that redraw must be caught
            // by the drain below along with any buffered `history -s` echoes — otherwise
            // it leaks through as a stray extra prompt once passthrough starts forwarding.
            // SIGWINCH delivery and the resulting redraw are asynchronous, so give the
            // shell a moment to actually process it before draining (same reasoning as
            // the cd-sync sleep below).
            if let Ok((cols, rows)) = crossterm::terminal::size() {
                sub.resize(cols, rows);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            sub.drain();
            // Only sync cwd if the subshell is not already in the panel's directory.
            let shell_cwd = std::fs::read_link(format!("/proc/{}/cwd", sub.child_pid))
                .ok()
                .map(|p| p.to_string_lossy().to_string());
            if shell_cwd.as_deref() != Some(cwd.as_str()) {
                // Sync silently: inject the cd, then discard its echo and the shell's
                // next prompt redraw below, so the user just sees one correctly-synced
                // real prompt instead of the cd being typed out at the old one.
                let _ = sub.send_line(&format!(" cd {}", shell_escape_path(&cwd)));
                // Give the shell a moment to run the cd and redraw its prompt, then
                // discard that before anything reaches the live screen.
                std::thread::sleep(std::time::Duration::from_millis(50));
                sub.drain();
                // Force a fresh real prompt now that the previous one was drained away.
                if cmdline_text.is_empty() {
                    let _ = sub.send_line("");
                } else {
                    let _ = sub.send_line("\x15");
                    sub.send_raw(cmdline_text.as_bytes());
                }
            } else if self.subshell_prompt_needed {
                // A sc cmdline command ran since last passthrough: drain() discarded
                // bash's PS1, so we need \n to force a fresh one. \x15 is only needed
                // when cmdline text will follow (see above); otherwise it just rings
                // the bell for no reason on an already-empty line.
                self.subshell_prompt_needed = false;
                if cmdline_text.is_empty() {
                    let _ = sub.send_line("");
                } else {
                    let _ = sub.send_line("\x15");
                    sub.send_raw(cmdline_text.as_bytes());
                }
            } else if !cmdline_text.is_empty() {
                // Shell is idle at its prompt. \x15 clears any existing readline
                // content before we inject the SC cmdline text.
                sub.send_raw(b"\x15");
                sub.send_raw(cmdline_text.as_bytes());
            }
            pending_ipc = sub.start_passthrough(ipc_fd).unwrap_or_default();

            if !sub.is_alive() {
                // The user typed `exit` (or the shell crashed). Drop the Subshell
                // (which SIGTERMs and closes the master fd) and fall back to
                // stateless mode so sc keeps working normally.
                self.shell_mode = ShellMode::Stateless;
                // Skip cwd sync — /proc/<pid>/cwd is gone.
            } else if !matches!(self.active_panel().content, PanelContent::Matches(_)) {
                // Sync active panel to wherever the subshell ended up. Skipped for a
                // Matches panel: it isn't a directory browser, so there's no path to
                // navigate it to, and doing so anyway would reset its cursor/scroll.
                if let Ok(link) = std::fs::read_link(format!("/proc/{}/cwd", sub.child_pid)) {
                    let new_cwd = link.to_string_lossy().to_string();
                    let panel_cwd = self.active_panel().path.0.clone();
                    if new_cwd != panel_cwd {
                        if matches!(self.active_panel().content, PanelContent::SearchResults(_)) {
                            self.cache_search_to_history();
                            let panel = self.active_panel_mut();
                            panel.content = PanelContent::Dir;
                        }
                        self.navigate_to_path(&new_cwd);
                    }
                }
            }
        }

        for raw in pending_ipc {
            if let Some(msg) = parse_message(&raw) {
                self.handle_ipc(msg);
            }
        }

        // Return to TUI
        let _ = enable_raw_mode();
        if self.mouse {
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste);
        } else {
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen, EnableBracketedPaste);
        }
        // Refresh panels in case the subshell changed the filesystem
        self.needs_full_redraw = true;
        self.left.refresh();
        self.right.refresh();
    }
}

/// Resolves which `panels.default_action*` template applies to a file with the given
/// executable-bit state: `default_action_executable`/`default_action_text` if non-empty,
/// else `default_action` (which may itself be empty, meaning no-op).
pub(super) fn resolve_default_action_template(executable: bool, cfg: &crate::config::PanelsConfig) -> &str {
    let specific = if executable { &cfg.default_action_executable } else { &cfg.default_action_text };
    if !specific.is_empty() { specific } else { &cfg.default_action }
}

/// True when a user-menu command template uses a macro that reads the
/// inactive panel (%F, %D, %T, %U, %S); %% escapes are skipped.
pub(super) fn template_references_inactive(template: &str) -> bool {
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('F') | Some('D') | Some('T') | Some('U') | Some('S') => return true,
                _ => {}
            }
        }
    }
    false
}

/// Returns true if the `keys` shortcut string of a menu item matches `event`.
/// Returns true when `item.keys` matches the given key event.
/// `pending` is the first key of an in-progress chord, if any.
/// - `Single(ke)`:  matches when pending is None and event == ke
/// - `Chord(f, s)`: matches when pending == Some(f) and event == s
pub(super) fn menu_item_matches_key(
    item: &crate::config::MenuItem,
    pending: Option<&KeyEvent>,
    event: &KeyEvent,
) -> bool {
    let Some(keys) = &item.keys else { return false };
    match crate::config::parse_key_binding(keys) {
        Ok(KeyBinding::Single(ke)) => pending.is_none() && ke == *event,
        Ok(KeyBinding::Chord(f, s)) => {
            (pending == Some(&f)) && s == *event
        }
        _ => false,
    }
}

/// Returns true when `event` is the first key of a chord shortcut defined in any menu item.
pub(super) fn menu_item_is_chord_start(
    items: &[crate::config::MenuItem],
    event: &KeyEvent,
) -> bool {
    items.iter().any(|item| {
        let Some(keys) = &item.keys else { return false };
        matches!(
            crate::config::parse_key_binding(keys),
            Ok(KeyBinding::Chord(f, _)) if f == *event
        )
    })
}
