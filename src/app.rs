use std::cell::Cell;
use std::io::stdout;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    widgets::Block,
    Frame, Terminal,
};

use crate::config::{ActionBindings, Config, KeyBinding};
use crate::history::CommandHistory;
use crate::macros::{MacroContext, PanelContext};
use crate::provider::{NodeKind, NodePath, TreeProvider};
use crate::provider::filesystem::FilesystemProvider;
use crate::state::{AppState, Orientation};
use crate::ui::button::Button;
use crate::ui::button_bar::ButtonBarWidget;
use crate::ui::cmdline::{CmdLineState, CmdLineWidget};
use crate::ui::dialog::{render_confirm, render_error, ConfirmButtonAreas, ConfirmOp, ConfirmState, ErrorButtonArea};
use crate::ui::menu::{UserMenuAreas, UserMenuState, UserMenuWidget};
use crate::ui::output_overlay::OutputOverlayWidget;
use crate::ui::panel::{PanelState, PanelWidget};

// ── Mode enums ────────────────────────────────────────────────────────────────

pub enum ShellMode {
    Stateless,
}

pub enum AppMode {
    Ui,
}

// ── Startup path resolution ───────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub struct StartupPaths {
    pub left: PathBuf,
    pub right: PathBuf,
}

pub fn resolve_startup_paths(
    dir1: Option<&Path>,
    dir2: Option<&Path>,
    flag: Option<bool>,
    restore_paths_config: bool,
    saved: Option<(&Path, &Path)>,
    cwd: &Path,
) -> StartupPaths {
    if let Some(d1) = dir1 {
        let right = dir2.unwrap_or(d1);
        return StartupPaths {
            left: d1.to_path_buf(),
            right: right.to_path_buf(),
        };
    }
    let restore = flag.unwrap_or(restore_paths_config);
    if restore {
        if let Some((left, right)) = saved {
            return StartupPaths {
                left: left.to_path_buf(),
                right: right.to_path_buf(),
            };
        }
    }
    StartupPaths {
        left: cwd.to_path_buf(),
        right: cwd.to_path_buf(),
    }
}

// ── Side ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

// ── Action ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum Action {
    SwitchPanel,
    ToggleLayout,
    TagFile,
    InvertTags,
    Copy,
    Move,
    Delete,
    UserMenu,
    Exit,
    CmdlineInsertFilename,
    CmdlineInsertFullpath,
    CmdlineComplete,
    CmdlineInsertTagged,
    CmdlineInsertTaggedOther,
    CmdlineInsertPath,
    CmdlineInsertPathOther,
    ToggleShell,
    ToggleCmdline,
    ToggleButtonBar,
    CmdlineHistoryPrev,
    CmdlineHistoryNext,
}

// ── KeyMatch ──────────────────────────────────────────────────────────────────

enum KeyMatch {
    Act(Action),
    ChordStart,
    None,
}

fn match_key(
    bindings_list: &[(&ActionBindings, Action)],
    event: &KeyEvent,
    pending: Option<&KeyEvent>,
) -> KeyMatch {
    if let Some(first) = pending {
        for (bindings, action) in bindings_list {
            for b in *bindings {
                if let KeyBinding::Chord(f, s) = b {
                    if f == first && s == event {
                        return KeyMatch::Act(*action);
                    }
                }
            }
        }
        return KeyMatch::None;
    }
    for (bindings, action) in bindings_list {
        for b in *bindings {
            match b {
                KeyBinding::Single(ke) if ke == event => return KeyMatch::Act(*action),
                KeyBinding::Chord(first, _) if first == event => return KeyMatch::ChordStart,
                _ => {}
            }
        }
    }
    KeyMatch::None
}

fn event_matches_bindings(bindings: &ActionBindings, event: &KeyEvent) -> bool {
    bindings.iter().any(|b| matches!(b, KeyBinding::Single(ke) if ke == event))
}

// ── Modal ─────────────────────────────────────────────────────────────────────

enum Modal {
    None,
    UserMenu(UserMenuState),
    Confirm(ConfirmState),
    Error(String),
}

// ── ModalAreas ────────────────────────────────────────────────────────────────

enum ModalAreas {
    None,
    Confirm(ConfirmButtonAreas),
    UserMenu(UserMenuAreas),
    Error(ErrorButtonArea),
}

// ── AppLayout ─────────────────────────────────────────────────────────────────

struct AppLayout {
    left: Rect,
    right: Rect,
    panel_area: Rect,
    cmdline: Option<Rect>,
    button_bar: Option<Rect>,
}

impl AppLayout {
    fn compute(area: Rect, orientation: Orientation, show_cmdline: bool, show_button_bar: bool) -> Self {
        let mut constraints = vec![Constraint::Fill(1)];
        if show_cmdline {
            constraints.push(Constraint::Length(1));
        }
        if show_button_bar {
            constraints.push(Constraint::Length(1));
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        let panel_area = chunks[0];
        let mut idx = 1usize;
        let cmdline = if show_cmdline {
            let a = chunks[idx];
            idx += 1;
            Some(a)
        } else {
            None
        };
        let button_bar = if show_button_bar {
            Some(chunks[idx])
        } else {
            None
        };

        let (left, right) = match orientation {
            Orientation::Vertical => {
                let p = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(panel_area);
                (p[0], p[1])
            }
            Orientation::Horizontal => {
                let p = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(panel_area);
                (p[0], p[1])
            }
        };

        AppLayout { left, right, panel_area, cmdline, button_bar }
    }
}

// ── TerminalGuard ─────────────────────────────────────────────────────────────

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    config: Config,
    orientation: Orientation,
    show_cmdline: bool,
    show_button_bar: bool,
    left: PanelState,
    right: PanelState,
    active: Side,
    cmdline: CmdLineState,
    history: CommandHistory,
    last_output: Option<String>,
    show_output: bool,
    output_scroll: u16,
    modal: Modal,
    pending_chord: Option<KeyEvent>,
    last_click: Option<(Instant, u16, u16)>,
    // Updated during render for mouse hit-testing
    left_area: Cell<Rect>,
    right_area: Cell<Rect>,
    button_bar_area: Cell<Rect>,
    overlay_area: Cell<Rect>,
    // Modal button hit-test areas (updated each render)
    confirm_yes_btn: Cell<Button>,
    confirm_no_btn: Cell<Button>,
    error_ok_btn: Cell<Button>,
    menu_close_btn: Cell<Button>,
    menu_list_area: Cell<Rect>,
    menu_list_offset: Cell<usize>,
    // Pending left-button press for down+up click detection
    mouse_pressed: Option<Position>,
    should_quit: bool,
    mouse: bool,
}

impl App {
    pub fn new(config: Config, left_path: PathBuf, right_path: PathBuf, state: &AppState, mouse: bool) -> Self {
        let left_node = NodePath(left_path.to_string_lossy().into_owned());
        let right_node = NodePath(right_path.to_string_lossy().into_owned());

        let history = crate::state::history_path();
        let hist = if history.exists() {
            CommandHistory::load(&history).unwrap_or_else(|_| CommandHistory::new())
        } else {
            CommandHistory::new()
        };

        App {
            orientation: state.orientation,
            show_cmdline: state.show_cmdline,
            show_button_bar: state.show_button_bar,
            left: PanelState::new(Box::new(FilesystemProvider), left_node),
            right: PanelState::new(Box::new(FilesystemProvider), right_node),
            active: Side::Left,
            cmdline: CmdLineState::new(),
            history: hist,
            last_output: None,
            show_output: false,
            output_scroll: 0,
            modal: Modal::None,
            pending_chord: None,
            last_click: None,
            left_area: Cell::new(Rect::default()),
            right_area: Cell::new(Rect::default()),
            button_bar_area: Cell::new(Rect::default()),
            overlay_area: Cell::new(Rect::default()),
            confirm_yes_btn: Cell::new(Button::default()),
            confirm_no_btn: Cell::new(Button::default()),
            error_ok_btn: Cell::new(Button::default()),
            menu_close_btn: Cell::new(Button::default()),
            menu_list_area: Cell::new(Rect::default()),
            menu_list_offset: Cell::new(0),
            mouse_pressed: None,
            should_quit: false,
            mouse,
            config,
        }
    }

    fn active_panel(&self) -> &PanelState {
        match self.active {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn inactive_panel(&self) -> &PanelState {
        match self.active {
            Side::Left => &self.right,
            Side::Right => &self.left,
        }
    }

    fn active_panel_mut(&mut self) -> &mut PanelState {
        match self.active {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    fn inactive_panel_mut(&mut self) -> &mut PanelState {
        match self.active {
            Side::Left => &mut self.right,
            Side::Right => &mut self.left,
        }
    }

    fn panel_visible_height(&self, side: Side) -> usize {
        let area = match side {
            Side::Left => self.left_area.get(),
            Side::Right => self.right_area.get(),
        };
        area.height.saturating_sub(2).max(1) as usize
    }

    fn active_vh(&self) -> usize {
        self.panel_visible_height(self.active)
    }

    fn bindings_list(&self) -> [(&ActionBindings, Action); 21] {
        let kb = &self.config.keybindings;
        [
            (&kb.switch_panel, Action::SwitchPanel),
            (&kb.toggle_layout, Action::ToggleLayout),
            (&kb.tag_file, Action::TagFile),
            (&kb.invert_tags, Action::InvertTags),
            (&kb.copy, Action::Copy),
            (&kb.move_entry, Action::Move),
            (&kb.delete, Action::Delete),
            (&kb.user_menu, Action::UserMenu),
            (&kb.exit, Action::Exit),
            (&kb.cmdline_insert_filename, Action::CmdlineInsertFilename),
            (&kb.cmdline_insert_fullpath, Action::CmdlineInsertFullpath),
            (&kb.cmdline_complete, Action::CmdlineComplete),
            (&kb.cmdline_insert_tagged, Action::CmdlineInsertTagged),
            (&kb.cmdline_insert_tagged_other, Action::CmdlineInsertTaggedOther),
            (&kb.cmdline_insert_path, Action::CmdlineInsertPath),
            (&kb.cmdline_insert_path_other, Action::CmdlineInsertPathOther),
            (&kb.toggle_shell, Action::ToggleShell),
            (&kb.toggle_cmdline, Action::ToggleCmdline),
            (&kb.toggle_button_bar, Action::ToggleButtonBar),
            (&kb.cmdline_history_prev, Action::CmdlineHistoryPrev),
            (&kb.cmdline_history_next, Action::CmdlineHistoryNext),
        ]
    }

    fn handle_action(&mut self, action: Action) {
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
                if self.show_output {
                    self.show_output = false;
                }
                // Stateless mode: Ctrl+O hides output overlay
            }
            Action::UserMenu => {
                if self.config.menu.is_empty() {
                    self.modal = Modal::Error("No user menu entries configured.".to_string());
                } else {
                    self.modal = Modal::UserMenu(UserMenuState::new(self.config.menu.clone()));
                }
            }
            Action::TagFile => {
                let vh = self.active_vh();
                self.active_panel_mut().tag_toggle(vh);
            }
            Action::InvertTags => {
                if self.cmdline.is_empty() {
                    self.active_panel_mut().invert_tags();
                } else {
                    self.cmdline.insert_char('*');
                }
            }
            Action::Copy => {
                let files = self.active_panel().op_files();
                if files.is_empty() {
                    return;
                }
                let dst = self.inactive_panel().path.0.clone();
                let src = self.active_panel().path.0.clone();
                if src == dst {
                    self.modal = Modal::Error("Source and destination are the same directory.".to_string());
                    return;
                }
                self.modal = Modal::Confirm(ConfirmState {
                    op: ConfirmOp::Copy,
                    files,
                    dst: Some(dst),
                });
            }
            Action::Move => {
                let files = self.active_panel().op_files();
                if files.is_empty() {
                    return;
                }
                let dst = self.inactive_panel().path.0.clone();
                let src = self.active_panel().path.0.clone();
                if src == dst {
                    self.modal = Modal::Error("Source and destination are the same directory.".to_string());
                    return;
                }
                self.modal = Modal::Confirm(ConfirmState {
                    op: ConfirmOp::Move,
                    files,
                    dst: Some(dst),
                });
            }
            Action::Delete => {
                let files = self.active_panel().op_files();
                if !files.is_empty() {
                    self.modal = Modal::Confirm(ConfirmState {
                        op: ConfirmOp::Delete,
                        files,
                        dst: None,
                    });
                }
            }
            Action::CmdlineInsertFilename => {
                let name = self.active_panel().current_name();
                if !name.is_empty() && name != ".." {
                    self.cmdline.insert_str(&name);
                }
            }
            Action::CmdlineInsertFullpath => {
                let panel = self.active_panel();
                let name = panel.current_name();
                if !name.is_empty() && name != ".." {
                    let full = format!("{}/{}", panel.path.0, name);
                    self.cmdline.insert_str(&full);
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
                let s = names.join(" ");
                if !s.is_empty() {
                    self.cmdline.insert_str(&s);
                }
            }
            Action::CmdlineInsertTaggedOther => {
                let panel = self.inactive_panel();
                let mut names: Vec<String> = panel.tagged.iter().cloned().collect();
                if names.is_empty() {
                    if let Some(e) = panel.current_entry().filter(|e| e.name != "..") {
                        names.push(e.name.clone());
                    }
                }
                names.sort();
                let s = names.join(" ");
                if !s.is_empty() {
                    self.cmdline.insert_str(&s);
                }
            }
            Action::CmdlineInsertPath => {
                let path = self.active_panel().path.0.clone();
                self.cmdline.insert_str(&path);
            }
            Action::CmdlineInsertPathOther => {
                let path = self.inactive_panel().path.0.clone();
                self.cmdline.insert_str(&path);
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
                self.run_completion();
            }
        }
    }

    fn run_completion(&mut self) {
        let script = find_complete_script();
        let Some(script) = script else { return };
        let cmdline = self.cmdline.text.clone();
        let cursor = self.cmdline.cursor;
        if let Ok(out) = std::process::Command::new("bash")
            .arg(&script)
            .arg(&cmdline)
            .arg(cursor.to_string())
            .output()
        {
            if out.status.success() {
                let completions: Vec<&str> = std::str::from_utf8(&out.stdout)
                    .unwrap_or("")
                    .lines()
                    .filter(|s| !s.is_empty())
                    .collect();
                if completions.len() == 1 {
                    // Replace word under cursor
                    let word_start = cmdline[..cursor].rfind(' ').map(|i| i + 1).unwrap_or(0);
                    let mut new_text = cmdline[..word_start].to_string();
                    new_text.push_str(completions[0]);
                    let new_cursor = new_text.len();
                    new_text.push_str(&cmdline[cursor..]);
                    self.cmdline.text = new_text;
                    self.cmdline.cursor = new_cursor;
                } else if completions.len() > 1 {
                    let output = completions.join("\n");
                    self.last_output = Some(output);
                    self.show_output = true;
                    self.output_scroll = 0;
                }
            }
        }
    }

    fn execute_command(&mut self) {
        let cmd = self.cmdline.text.clone();
        if cmd.is_empty() {
            return;
        }
        self.history.push(cmd.clone());
        let _ = self.history.save(&crate::state::history_path());

        let cwd = self.active_panel().path.0.clone();
        match std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&cwd)
            .output()
        {
            Ok(out) => {
                let mut combined = String::new();
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push_str("\n--- stderr ---\n");
                    }
                    combined.push_str(&stderr);
                }
                if combined.is_empty() {
                    combined = "(no output)".to_string();
                }
                self.last_output = Some(combined);
                self.show_output = true;
                self.output_scroll = 0;
            }
            Err(e) => {
                self.last_output = Some(format!("Error running command: {}", e));
                self.show_output = true;
                self.output_scroll = 0;
            }
        }

        self.cmdline.clear();
        self.active_panel_mut().refresh();
    }

    fn execute_file_op(&mut self, state: ConfirmState) {
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
                }
            }
        }

        self.left.refresh();
        self.right.refresh();

        if !errors.is_empty() {
            self.modal = Modal::Error(errors.join("\n"));
        } else {
            self.modal = Modal::None;
        }
    }

    fn handle_key_event(&mut self, event: KeyEvent) {
        // Output overlay active — handle scroll keys
        if self.show_output {
            let dismiss = event.code == KeyCode::Esc
                || event_matches_bindings(&self.config.keybindings.toggle_shell, &event);
            if dismiss {
                self.show_output = false;
                return;
            }
            match event.code {
                KeyCode::Up => {
                    self.output_scroll = self.output_scroll.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    self.output_scroll = self.output_scroll.saturating_add(1);
                    return;
                }
                KeyCode::PageUp => {
                    self.output_scroll = self.output_scroll.saturating_sub(20);
                    return;
                }
                KeyCode::PageDown => {
                    self.output_scroll = self.output_scroll.saturating_add(20);
                    return;
                }
                _ => {}
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
                let confirmed = matches!(
                    event.code,
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
                );
                let cancelled = matches!(
                    event.code,
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc
                );
                if confirmed {
                    if let Modal::Confirm(state) = std::mem::replace(&mut self.modal, Modal::None) {
                        self.execute_file_op(state);
                    }
                } else if cancelled {
                    self.modal = Modal::None;
                }
                return;
            }
            Modal::UserMenu(_) => {
                match event.code {
                    KeyCode::Esc => {
                        self.modal = Modal::None;
                    }
                    KeyCode::Up => {
                        if let Modal::UserMenu(ref mut s) = self.modal {
                            s.move_up();
                        }
                    }
                    KeyCode::Down => {
                        if let Modal::UserMenu(ref mut s) = self.modal {
                            s.move_down();
                        }
                    }
                    KeyCode::Enter => {
                        if let Modal::UserMenu(ref s) = self.modal {
                            if let Some(item) = s.selected() {
                                let cmd_template = item.command.clone();
                                self.modal = Modal::None;
                                let result = self.expand_menu_command(&cmd_template);
                                if result.untag_active {
                                    self.active_panel_mut().tagged.clear();
                                }
                                if result.untag_inactive {
                                    self.inactive_panel_mut().tagged.clear();
                                }
                                self.cmdline.text = result.text;
                                self.cmdline.move_end();
                                self.execute_command();
                            }
                        }
                    }
                    _ => {}
                }
                return;
            }
        }

        // Chord handling
        // Pre-dispatch: plain Delete on a non-empty cmdline is text editing, not a file op.
        if event.code == KeyCode::Delete
            && event.modifiers == KeyModifiers::NONE
            && !self.cmdline.is_empty()
        {
            self.cmdline.delete_char();
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
            KeyMatch::None => {}
        }

        // Raw key handling
        match event.code {
            KeyCode::Char(c) if event.modifiers == KeyModifiers::NONE
                || event.modifiers == KeyModifiers::SHIFT =>
            {
                self.cmdline.insert_char(c);
            }
            KeyCode::Backspace if event.modifiers == KeyModifiers::NONE => {
                self.cmdline.backspace();
            }
            KeyCode::Delete if event.modifiers == KeyModifiers::NONE => {
                // Non-empty cmdline is handled pre-dispatch above; here cmdline is always empty.
                let files = self.active_panel().op_files();
                if !files.is_empty() {
                    self.modal = Modal::Confirm(ConfirmState {
                        op: ConfirmOp::Delete,
                        files,
                        dst: None,
                    });
                }
            }
            KeyCode::Left if event.modifiers == KeyModifiers::NONE => {
                self.cmdline.move_left();
            }
            KeyCode::Right if event.modifiers == KeyModifiers::NONE => {
                self.cmdline.move_right();
            }
            KeyCode::Home if event.modifiers == KeyModifiers::NONE => {
                if !self.cmdline.is_empty() {
                    self.cmdline.move_home();
                } else {
                    let vh = self.active_vh();
                    self.active_panel_mut().move_cursor(i32::MIN, vh);
                }
            }
            KeyCode::End if event.modifiers == KeyModifiers::NONE => {
                if !self.cmdline.is_empty() {
                    self.cmdline.move_end();
                } else {
                    let vh = self.active_vh();
                    self.active_panel_mut().move_cursor(i32::MAX / 2, vh);
                }
            }
            KeyCode::Up if event.modifiers == KeyModifiers::NONE => {
                let vh = self.active_vh();
                self.active_panel_mut().move_cursor(-1, vh);
            }
            KeyCode::Down if event.modifiers == KeyModifiers::NONE => {
                let vh = self.active_vh();
                self.active_panel_mut().move_cursor(1, vh);
            }
            KeyCode::PageUp if event.modifiers == KeyModifiers::NONE => {
                let vh = self.active_vh();
                self.active_panel_mut().move_cursor(-(vh as i32), vh);
            }
            KeyCode::PageDown if event.modifiers == KeyModifiers::NONE => {
                let vh = self.active_vh();
                self.active_panel_mut().move_cursor(vh as i32, vh);
            }
            KeyCode::Enter if event.modifiers == KeyModifiers::NONE => {
                if !self.cmdline.is_empty() {
                    self.execute_command();
                } else {
                    let entry = self.active_panel().current_entry();
                    if entry.map(|e| e.kind == NodeKind::Dir).unwrap_or(false) {
                        self.active_panel_mut().enter_dir();
                    }
                }
            }
            KeyCode::Esc if event.modifiers == KeyModifiers::NONE && self.show_output => {
                self.show_output = false;
            }
            _ => {}
        }
    }

    fn expand_menu_command(&self, template: &str) -> crate::macros::ExpandResult {
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

    // Called on left-button Down inside a modal: only visual updates (no actions).
    fn handle_modal_down(&mut self, col: u16, row: u16) {
        let list_area = self.menu_list_area.get();
        let list_offset = self.menu_list_offset.get();
        let pos = Position { x: col, y: row };
        if list_area.contains(pos) {
            let item_idx = (row - list_area.y) as usize + list_offset;
            if let Modal::UserMenu(ref mut s) = self.modal {
                if item_idx < s.items.len() {
                    s.cursor = item_idx;
                }
            }
        }
    }

    // Called on every Left-button Up inside a modal; fires actions only when
    // `up` matches the stored Down position AND lands on a button (via Button::clicked).
    fn handle_modal_click(&mut self, up: Position) {
        let yes_btn = self.confirm_yes_btn.get();
        let no_btn = self.confirm_no_btn.get();
        let ok_btn = self.error_ok_btn.get();
        let close_btn = self.menu_close_btn.get();
        let list_area = self.menu_list_area.get();
        let list_offset = self.menu_list_offset.get();
        let down = self.mouse_pressed;

        // Pre-extract menu item command to avoid nested borrows.
        let menu_item_cmd: Option<String> =
            if matches!(self.modal, Modal::UserMenu(_))
                && down == Some(up)
                && list_area.contains(up)
                && !close_btn.contains(up)
            {
                let item_idx = (up.y - list_area.y) as usize + list_offset;
                if let Modal::UserMenu(ref s) = self.modal {
                    s.items.get(item_idx).map(|i| i.command.clone())
                } else {
                    None
                }
            } else {
                None
            };

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
            Modal::UserMenu(_) => {
                if close_btn.clicked(down, up) {
                    self.modal = Modal::None;
                } else if let Some(cmd_template) = menu_item_cmd {
                    self.modal = Modal::None;
                    let result = self.expand_menu_command(&cmd_template);
                    if result.untag_active {
                        self.active_panel_mut().tagged.clear();
                    }
                    if result.untag_inactive {
                        self.inactive_panel_mut().tagged.clear();
                    }
                    self.cmdline.text = result.text;
                    self.cmdline.move_end();
                    self.execute_command();
                }
            }
        }
    }

    fn handle_button_bar_click(&mut self, pos: Position) {
        let bb_area = self.button_bar_area.get();
        if let Some(n) = ButtonBarWidget::button_at(&self.config.keybindings, bb_area.x, pos) {
            self.handle_key_event(KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE));
        }
    }

    fn handle_panel_down(&mut self, col: u16, row: u16, btn: MouseButton) {
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

        let inner_y = clicked_area.y + 1;
        if row < inner_y || row >= clicked_area.y + clicked_area.height - 1 {
            return;
        }
        let entry_row = (row - inner_y) as usize;
        let vh = clicked_area.height.saturating_sub(2).max(1) as usize;

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
                if clicked_side != self.active {
                    self.active = clicked_side;
                }
                let panel = match clicked_side {
                    Side::Left => &mut self.left,
                    Side::Right => &mut self.right,
                };
                panel.move_cursor_to_row(entry_row, vh);
                if is_double {
                    let entry = panel.current_entry();
                    if entry.map(|e| e.kind == NodeKind::Dir).unwrap_or(false) {
                        panel.enter_dir();
                    }
                }
            }
            MouseButton::Right => {
                if clicked_side != self.active {
                    self.active = clicked_side;
                }
                let panel = match clicked_side {
                    Side::Left => &mut self.left,
                    Side::Right => &mut self.right,
                };
                panel.move_cursor_to_row(entry_row, vh);
                panel.tag_toggle(vh);
            }
            MouseButton::Middle => {}
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        let pos = Position { x: col, y: row };

        // Output overlay scroll support (fires on Down)
        if self.show_output {
            let area = self.overlay_area.get();
            if area.contains(pos) {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        self.output_scroll = self.output_scroll.saturating_sub(3);
                        return;
                    }
                    MouseEventKind::ScrollDown => {
                        self.output_scroll = self.output_scroll.saturating_add(3);
                        return;
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        let inner_x = area.x + 1;
                        let inner_y = area.y + 1;
                        let inner_w = area.width.saturating_sub(2);
                        let inner_h = area.height.saturating_sub(2);
                        let scrollbar_col = inner_x + inner_w;
                        if col == scrollbar_col.saturating_sub(1) && inner_h > 0 {
                            let total_lines = self.last_output.as_deref()
                                .map(|t| t.lines().count())
                                .unwrap_or(0);
                            let track_row = row.saturating_sub(inner_y) as usize;
                            let new_pos = track_row * total_lines / inner_h as usize;
                            self.output_scroll = new_pos as u16;
                        }
                        return;
                    }
                    _ => { return; }
                }
            }
        }

        // Modals capture all mouse events.
        // Down: store press + visual update. Up: delegate to handle_modal_click
        // unconditionally — Button::clicked enforces the Down==Up requirement.
        if !matches!(self.modal, Modal::None) {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    if let Modal::UserMenu(ref mut s) = self.modal { s.move_up(); }
                }
                MouseEventKind::ScrollDown => {
                    if let Modal::UserMenu(ref mut s) = self.modal { s.move_down(); }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.mouse_pressed = Some(pos);
                    self.handle_modal_down(col, row);
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.handle_modal_click(pos);
                    self.mouse_pressed = None;
                }
                _ => {}
            }
            return;
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
                if self.mouse_pressed == Some(pos) {
                    if self.show_button_bar {
                        let bb = self.button_bar_area.get();
                        if bb.contains(pos) {
                            self.handle_button_bar_click(pos);
                        }
                    }
                }
                self.mouse_pressed = None;
            }
            // Right-click on panel fires immediately on Down.
            MouseEventKind::Down(btn) => {
                self.handle_panel_down(col, row, btn);
            }
            _ => {}
        }
    }

    fn save_state(&self) {
        let state = AppState {
            orientation: self.orientation,
            show_cmdline: self.show_cmdline,
            show_button_bar: self.show_button_bar,
            left_path: self.left.path.0.clone(),
            right_path: self.right.path.0.clone(),
        };
        let _ = state.save();
        let _ = self.history.save(&crate::state::history_path());
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let press = self.mouse_pressed;
        let layout = AppLayout::compute(
            area,
            self.orientation,
            self.show_cmdline,
            self.show_button_bar,
        );

        self.left_area.set(layout.left);
        self.right_area.set(layout.right);

        // Clone colorscheme so we can borrow panel states mutably without aliasing issues.
        let cs = self.config.colorscheme.clone();

        // Background
        let bg = Color::Rgb(cs.panel_bg.0, cs.panel_bg.1, cs.panel_bg.2);
        frame.render_widget(
            Block::default().style(Style::default().bg(bg)),
            area,
        );

        // Panels
        let left_active = self.active == Side::Left;
        let left_title = self.left.path.0.clone();
        frame.render_stateful_widget(
            PanelWidget { cs: &cs, active: left_active, title: left_title },
            layout.left,
            &mut self.left,
        );
        let right_active = self.active == Side::Right;
        let right_title = self.right.path.0.clone();
        frame.render_stateful_widget(
            PanelWidget { cs: &cs, active: right_active, title: right_title },
            layout.right,
            &mut self.right,
        );

        // Output overlay
        if self.show_output {
            if let Some(text) = &self.last_output {
                self.overlay_area.set(layout.panel_area);
                let overlay = OutputOverlayWidget {
                    cs: &cs,
                    text,
                    scroll: self.output_scroll,
                };
                frame.render_widget(overlay, layout.panel_area);
            }
        }

        // CmdLine
        if let Some(cmdline_area) = layout.cmdline {
            let widget = CmdLineWidget { cs: &cs, prompt: "$ " };
            let buf = frame.buffer_mut();
            let cursor_pos = widget.render_with_cursor(cmdline_area, buf, &self.cmdline);
            if let Some(pos) = cursor_pos {
                if matches!(self.modal, Modal::None) && !self.show_output {
                    frame.set_cursor_position(pos);
                }
            }
        }

        // Button bar
        if let Some(bb_area) = layout.button_bar {
            self.button_bar_area.set(bb_area);
            frame.render_widget(
                ButtonBarWidget { cs: &cs, kb: &self.config.keybindings, press },
                bb_area,
            );
        }

        // Modals (drawn last, on top) — capture returned hit-test areas
        let modal_areas = match &mut self.modal {
            Modal::None => ModalAreas::None,
            Modal::UserMenu(state) => {
                let a = UserMenuWidget { cs: &cs }.render_in(area, frame.buffer_mut(), state, press);
                ModalAreas::UserMenu(a)
            }
            Modal::Confirm(state) => {
                let a = render_confirm(area, frame.buffer_mut(), &cs, state, press);
                ModalAreas::Confirm(a)
            }
            Modal::Error(msg) => {
                let msg = msg.clone();
                let a = render_error(area, frame.buffer_mut(), &cs, &msg, press);
                ModalAreas::Error(a)
            }
        };
        // Borrow of self.modal released; store areas for mouse hit-testing
        match modal_areas {
            ModalAreas::None => {}
            ModalAreas::Confirm(a) => {
                self.confirm_yes_btn.set(a.yes);
                self.confirm_no_btn.set(a.no);
            }
            ModalAreas::UserMenu(a) => {
                self.menu_list_area.set(a.list_area);
                self.menu_list_offset.set(a.list_offset);
                self.menu_close_btn.set(a.close);
            }
            ModalAreas::Error(a) => {
                self.error_ok_btn.set(a.ok);
            }
        }
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        if self.mouse {
            execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        } else {
            execute!(stdout(), EnterAlternateScreen)?;
        }
        let _guard = TerminalGuard;

        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;

        loop {
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
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }

        self.save_state();
        Ok(())
    }
}

fn find_complete_script() -> Option<PathBuf> {
    let dev = Path::new("scripts/sc-complete");
    if dev.exists() {
        return Some(dev.to_path_buf());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("sc-complete");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }
    fn rp(
        dir1: Option<&str>,
        dir2: Option<&str>,
        flag: Option<bool>,
        config: bool,
        saved: Option<(&str, &str)>,
        cwd: &str,
    ) -> StartupPaths {
        resolve_startup_paths(
            dir1.map(Path::new),
            dir2.map(Path::new),
            flag,
            config,
            saved.map(|(l, r)| (Path::new(l), Path::new(r))),
            Path::new(cwd),
        )
    }

    #[test]
    fn no_args_config_false() {
        let r = rp(None, None, None, false, Some(("/saved/left", "/saved/right")), "/cwd");
        assert_eq!(r, StartupPaths { left: p("/cwd"), right: p("/cwd") });
    }

    #[test]
    fn no_args_config_true() {
        let r = rp(None, None, None, true, Some(("/saved/left", "/saved/right")), "/cwd");
        assert_eq!(r, StartupPaths { left: p("/saved/left"), right: p("/saved/right") });
    }

    #[test]
    fn no_args_restore_flag_overrides_config() {
        let r = rp(None, None, Some(true), false, Some(("/s/l", "/s/r")), "/cwd");
        assert_eq!(r, StartupPaths { left: p("/s/l"), right: p("/s/r") });
    }

    #[test]
    fn no_args_no_restore_flag_overrides_config() {
        let r = rp(None, None, Some(false), true, Some(("/s/l", "/s/r")), "/cwd");
        assert_eq!(r, StartupPaths { left: p("/cwd"), right: p("/cwd") });
    }

    #[test]
    fn dir1_only_sets_both_panels() {
        let r = rp(Some("/tmp"), None, None, false, None, "/cwd");
        assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/tmp") });
    }

    #[test]
    fn dir1_only_flag_ignored() {
        let r = rp(Some("/tmp"), None, Some(true), false, Some(("/s/l", "/s/r")), "/cwd");
        assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/tmp") });
    }

    #[test]
    fn dir1_and_dir2() {
        let r = rp(Some("/tmp"), Some("/var"), None, false, None, "/cwd");
        assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/var") });
    }

    #[test]
    fn dir1_and_dir2_flag_ignored() {
        let r = rp(Some("/tmp"), Some("/var"), Some(true), false, Some(("/s/l", "/s/r")), "/cwd");
        assert_eq!(r, StartupPaths { left: p("/tmp"), right: p("/var") });
    }

    #[test]
    fn restore_with_no_saved_state_falls_back_to_cwd() {
        let r = rp(None, None, Some(true), true, None, "/cwd");
        assert_eq!(r, StartupPaths { left: p("/cwd"), right: p("/cwd") });
    }
}
