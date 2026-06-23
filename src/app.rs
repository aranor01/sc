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
use crate::ui::popup_list::{PopupListState, PopupListWidget};
use crate::ui::dialog::{render_confirm, render_error, ConfirmButtonAreas, ConfirmOp, ConfirmState, ErrorButtonArea};
use crate::ui::menu::{UserMenuAreas, UserMenuState, UserMenuWidget};
use crate::ui::output_overlay::{OutputOverlayState, OutputOverlayWidget};
use crate::ui::modal_event::{CmdlineOutcome, ModalOutcome, OverlayOutcome, PanelOutcome, PopupOutcome};
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
    ReverseSearch,
    SyncPanels,
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


// ── Popup sessions ────────────────────────────────────────────────────────────

struct CompletionSession {
    list: PopupListState,
    word_start: usize,
}

impl CompletionSession {
    fn handle_key(&mut self, event: &KeyEvent) -> PopupOutcome {
        self.list.handle_key(event)
    }
}

struct ReverseSearchSession {
    list: PopupListState,
}

impl ReverseSearchSession {
    fn handle_key(&mut self, event: &KeyEvent) -> PopupOutcome {
        self.list.handle_key(event)
    }
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
    fn compute(area: Rect, orientation: Orientation, show_cmdline: bool, show_button_bar: bool, cmdline_height: u16) -> Self {
        let mut constraints = vec![Constraint::Fill(1)];
        if show_cmdline {
            constraints.push(Constraint::Length(cmdline_height));
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
    overlay: OutputOverlayState,
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
    // Popup list hit-test areas and scroll offsets (reset each frame when not visible)
    completion_popup_area: Cell<Rect>,
    completion_popup_offset: Cell<usize>,
    rev_search_popup_area: Cell<Rect>,
    rev_search_popup_offset: Cell<usize>,
    // Pending left-button press for down+up click detection
    mouse_pressed: Option<Position>,
    should_quit: bool,
    mouse: bool,
    // True when cmdline is empty OR user pressed ESC to give panels focus temporarily.
    // Cleared after the first keystroke (if cmdline is non-empty).
    explicit_action_mode: bool,
    completion: Option<CompletionSession>,
    reverse_search: Option<ReverseSearchSession>,
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
            overlay: OutputOverlayState::new(),
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
            completion_popup_area: Cell::new(Rect::default()),
            completion_popup_offset: Cell::new(0),
            rev_search_popup_area: Cell::new(Rect::default()),
            rev_search_popup_offset: Cell::new(0),
            mouse_pressed: None,
            should_quit: false,
            mouse,
            explicit_action_mode: false,
            completion: None,
            reverse_search: None,
            config,
        }
    }

    fn action_mode(&self) -> bool {
        self.cmdline.is_empty() || self.explicit_action_mode || self.pending_chord.is_some()
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

    fn bindings_list(&self) -> [(&ActionBindings, Action); 23] {
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
            (&kb.reverse_search, Action::ReverseSearch),
            (&kb.sync_panels, Action::SyncPanels),
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
                self.active_panel_mut().invert_tags();
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
                if !self.cmdline.text.trim().is_empty() {
                    let candidates = self.collect_candidates();
                    let word_start = last_word_start(&self.cmdline.text);
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
                    list: PopupListState { items, selected },
                });
            }
            Action::SyncPanels => {
                let path = self.active_panel().path.clone();
                let inactive = self.inactive_panel_mut();
                inactive.path = path;
                inactive.cursor = 0;
                inactive.scroll = 0;
                inactive.tagged.clear();
                inactive.refresh();
            }
        }
    }

    /// Call the completion script with the current cmdline text and return all candidates.
    fn collect_candidates(&self) -> Vec<String> {
        if self.cmdline.text.trim().is_empty() {
            return vec![];
        }
        let Some(script) = find_complete_script() else { return vec![]; };
        match std::process::Command::new("bash")
            .arg(&script)
            .arg(&self.cmdline.text)
            .output()
        {
            Ok(out) => std::str::from_utf8(&out.stdout)
                .unwrap_or("")
                .lines()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            Err(_) => vec![],
        }
    }

    /// Replace the last word in the cmdline with `candidate`.
    fn apply_word_replacement(&mut self, word_start: usize, candidate: &str) {
        self.cmdline.text.truncate(word_start);
        self.cmdline.cursor = word_start;
        self.cmdline.insert_str(candidate);
    }

    /// Apply the currently selected popup candidate and close the popup.
    fn apply_completion(&mut self) {
        if let Some(session) = self.completion.take() {
            if let Some(candidate) = session.list.items.get(session.list.selected).cloned() {
                self.apply_word_replacement(session.word_start, &candidate);
            }
        }
    }

    /// Re-run completion after a cmdline edit while the popup is open.
    /// Keeps the popup open (updating candidates) or closes it as needed.
    fn refresh_completion(&mut self) {
        if self.cmdline.text.trim().is_empty() {
            self.completion = None;
            return;
        }
        let candidates = self.collect_candidates();
        let word_start = last_word_start(&self.cmdline.text);
        match candidates.len() {
            0 => {
                self.completion = None;
            }
            1 => {
                let candidate = candidates[0].clone();
                self.apply_word_replacement(word_start, &candidate);
                self.completion = None;
            }
            _ => {
                if let Some(session) = &mut self.completion {
                    session.list.items = candidates;
                    session.word_start = word_start;
                    session.list.selected = 0;
                }
            }
        }
    }

    /// Re-filter history after a cmdline edit during reverse-search.
    /// Preserves the highlighted entry if it still appears in the new list.
    fn update_reverse_search(&mut self) {
        let Some(session) = &mut self.reverse_search else { return };
        let prev = session.list.items.get(session.list.selected).cloned();
        let new_items = history_matches(&self.history, &self.cmdline.text);
        let new_selected = prev
            .and_then(|p| new_items.iter().rposition(|s| *s == p))
            .unwrap_or_else(|| new_items.len().saturating_sub(1));
        session.list.items = new_items;
        session.list.selected = new_selected;
    }

    fn execute_menu_item(&mut self, cmd_template: String) {
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
                self.overlay = OutputOverlayState::new();
            }
            Err(e) => {
                self.last_output = Some(format!("Error running command: {}", e));
                self.show_output = true;
                self.overlay = OutputOverlayState::new();
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
        let was_explicit = self.explicit_action_mode;
        self.handle_key_event_inner(event);
        // One-shot: clear ESC-triggered action mode after the first key is processed,
        // but only if there is text in the cmdline (empty cmdline keeps auto-action-mode).
        if was_explicit && !self.cmdline.is_empty() {
            self.explicit_action_mode = false;
        }
    }

    fn handle_key_event_inner(&mut self, event: KeyEvent) {
        // Output overlay active — handle scroll keys
        if self.show_output {
            match self.overlay.handle_key(&event, &self.config.keybindings.toggle_shell) {
                OverlayOutcome::Dismissed => { self.show_output = false; return; }
                OverlayOutcome::Consumed => return,
                OverlayOutcome::Passthrough => {}
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
                let outcome = if let Modal::Confirm(ref s) = self.modal {
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
            Modal::UserMenu(_) => {
                let vh = self.menu_list_area.get().height as usize;
                let outcome = if let Modal::UserMenu(ref mut s) = self.modal {
                    s.handle_key(&event, vh)
                } else { ModalOutcome::Consumed };
                match outcome {
                    ModalOutcome::Execute(cmd) => {
                        self.modal = Modal::None;
                        self.execute_menu_item(cmd);
                    }
                    ModalOutcome::Dismissed => self.modal = Modal::None,
                    _ => {}
                }
                return;
            }
        }

        // Completion popup: intercept keys while a candidate list is visible.
        if let Some(ref mut session) = self.completion {
            match session.handle_key(&event) {
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
                        let ws = last_word_start(&self.cmdline.text);
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
            match session.handle_key(&event) {
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
                let cmd = self.config.menu.iter()
                    .find(|item| menu_item_matches_key(item, pending.as_ref(), &event))
                    .map(|item| item.command.clone());
                if let Some(cmd) = cmd {
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
        match self.active_panel_mut().handle_key(&event, vh, am) {
            PanelOutcome::Consumed => return,
            PanelOutcome::ExecuteCommand => { self.execute_command(); return; }
            PanelOutcome::Passthrough => {}
        }

        // Cmdline key handling
        match self.cmdline.handle_key(&event) {
            CmdlineOutcome::HistoryPrev => self.handle_action(Action::CmdlineHistoryPrev),
            CmdlineOutcome::HistoryNext => self.handle_action(Action::CmdlineHistoryNext),
            CmdlineOutcome::Consumed | CmdlineOutcome::Passthrough => {}
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
            if down == Some(up) && list_area.contains(up) && !close_btn.contains(up) {
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
        if let Some(n) = ButtonBarWidget::button_at(&self.config.keybindings, bb_area, pos) {
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

        // Output overlay scroll support. Skipped when a modal is open so that
        // modal button clicks are not swallowed by the overlay catch-all.
        if self.show_output && matches!(self.modal, Modal::None) {
            let area = self.overlay_area.get();
            if area.contains(pos) {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        self.overlay.scroll_by(-3);
                        return;
                    }
                    MouseEventKind::ScrollDown => {
                        self.overlay.scroll_by(3);
                        return;
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        let inner_y = area.y + 1;
                        let inner_w = area.width.saturating_sub(2);
                        let inner_h = area.height.saturating_sub(2) as usize;
                        let scrollbar_col = area.x + 1 + inner_w;
                        if col == scrollbar_col.saturating_sub(1) {
                            let total_lines = self.last_output.as_deref()
                                .map(|t| t.lines().count())
                                .unwrap_or(0);
                            let track_row = row.saturating_sub(inner_y) as usize;
                            self.overlay.scrollbar_click(track_row, inner_h, total_lines);
                        }
                        return;
                    }
                    MouseEventKind::Up(_) => {
                        self.mouse_pressed = None;
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

        // Popup lists (completion / reverse-i-search) intercept mouse events while visible.
        // Down outside the popup dismisses it (= ESC) and swallows the click.
        {
            let completion_area = self.completion_popup_area.get();
            let rev_search_area = self.rev_search_popup_area.get();

            match mouse.kind {
                MouseEventKind::Down(_) => {
                    if self.completion.is_some() && completion_area.width > 0 {
                        if completion_area.contains(pos) {
                            let inner_y = completion_area.y + 1;
                            if pos.y >= inner_y {
                                let row = (pos.y - inner_y) as usize;
                                let idx = self.completion_popup_offset.get() + row;
                                if let Some(s) = self.completion.as_mut() {
                                    if idx < s.list.items.len() { s.list.selected = idx; }
                                }
                            }
                            self.mouse_pressed = Some(pos);
                        } else {
                            self.completion = None;
                        }
                        return;
                    }
                    if self.reverse_search.is_some() && rev_search_area.width > 0 {
                        if rev_search_area.contains(pos) {
                            let inner_y = rev_search_area.y + 1;
                            if pos.y >= inner_y {
                                let row = (pos.y - inner_y) as usize;
                                let idx = self.rev_search_popup_offset.get() + row;
                                if let Some(s) = self.reverse_search.as_mut() {
                                    if idx < s.list.items.len() { s.list.selected = idx; }
                                }
                            }
                            self.mouse_pressed = Some(pos);
                        } else {
                            self.reverse_search = None;
                        }
                        return;
                    }
                }
                MouseEventKind::Up(_) => {
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
                if self.mouse_pressed == Some(pos) && self.show_button_bar {
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
                let vh = area.height.saturating_sub(2).max(1) as usize;
                let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) { -1 } else { 1 };
                panel.move_cursor(delta, vh);
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

        // Clone colorscheme so we can borrow panel states mutably without aliasing issues.
        let cs = self.config.colorscheme.clone();

        // Build the cmdline widget early so we can query needed_lines() before layout.
        let am = self.action_mode();
        let prompt = if self.reverse_search.is_some() { "(reverse-i-search): " } else { "$ " };
        let cmdline_widget = CmdLineWidget { cs: &cs, prompt, active: !am };
        let cmdline_height = if self.show_cmdline {
            cmdline_widget.needed_lines(&self.cmdline, area.width)
        } else {
            0
        };

        let layout = AppLayout::compute(
            area,
            self.orientation,
            self.show_cmdline,
            self.show_button_bar,
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
                    scroll: self.overlay.scroll,
                };
                frame.render_widget(overlay, layout.panel_area);
            }
        }

        // CmdLine
        if let Some(cmdline_area) = layout.cmdline {
            let buf = frame.buffer_mut();
            let cursor_pos = cmdline_widget.render_with_cursor(cmdline_area, buf, &self.cmdline);
            if let Some(pos) = cursor_pos {
                if matches!(self.modal, Modal::None) && !self.show_output && !am {
                    frame.set_cursor_position(pos);
                }
            }
        }

        // Popup list rendering (completion & reverse-search); shown only without modal/overlay
        if matches!(self.modal, Modal::None) && !self.show_output {
            if let Some(cmdline_area) = layout.cmdline {
                let width = cmdline_area.width as usize;

                if let Some(session) = self.completion.as_ref() {
                    if width > 0 {
                        let prompt_len = prompt.chars().count();
                        let anchor_byte = word_anchor_byte(&self.cmdline.text);
                        let anchor_chars = self.cmdline.text[..anchor_byte].chars().count();
                        let total_col = prompt_len + anchor_chars;
                        let anchor_x = cmdline_area.x + (total_col % width) as u16;
                        let anchor_y = cmdline_area.y + (total_col / width) as u16;
                        let (r, offset) = PopupListWidget { cs: &cs, state: &session.list }
                            .render_at(area, frame.buffer_mut(), anchor_x, anchor_y, self.completion_popup_offset.get());
                        self.completion_popup_area.set(r);
                        self.completion_popup_offset.set(offset);
                    }
                } else {
                    self.completion_popup_area.set(Rect::default());
                    self.completion_popup_offset.set(0);
                }

                if let Some(session) = self.reverse_search.as_ref() {
                    let (r, offset) = PopupListWidget { cs: &cs, state: &session.list }
                        .render_at(area, frame.buffer_mut(), cmdline_area.x, cmdline_area.y, self.rev_search_popup_offset.get());
                    self.rev_search_popup_area.set(r);
                    self.rev_search_popup_offset.set(offset);
                } else {
                    self.rev_search_popup_area.set(Rect::default());
                    self.rev_search_popup_offset.set(0);
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
                        if self.should_quit {
                            break;
                        }
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
    // Development build: look for scripts/sc-complete or scripts/complete.sh under CWD
    for name in &["scripts/sc-complete", "scripts/complete.sh"] {
        let p = Path::new(name);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    // Installed: look next to the binary
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

/// Returns true if the `keys` shortcut string of a menu item matches `event`.
/// Returns true when `item.keys` matches the given key event.
/// `pending` is the first key of an in-progress chord, if any.
/// - `Single(ke)`:  matches when pending is None and event == ke
/// - `Chord(f, s)`: matches when pending == Some(f) and event == s
fn menu_item_matches_key(
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
fn menu_item_is_chord_start(
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

/// All history entries that contain `filter` as a substring, oldest first.
/// If `filter` is empty, all entries are returned.
fn history_matches(history: &crate::history::CommandHistory, filter: &str) -> Vec<String> {
    if filter.is_empty() {
        history.entries().map(String::from).collect()
    } else {
        history.entries().filter(|s| s.contains(filter)).map(String::from).collect()
    }
}

/// Byte offset of the start of the last word in `text`.
/// Returns `text.len()` when the text ends with a space (empty current word).
fn last_word_start(text: &str) -> usize {
    if text.ends_with(' ') {
        text.len()
    } else {
        text.rfind(' ').map(|i| i + 1).unwrap_or(0)
    }
}

/// Byte offset used to anchor the completion popup:
/// - Start of the last word when text ends with a non-space character.
/// - Position of the last character (the space) when text ends with a space.
fn word_anchor_byte(text: &str) -> usize {
    if text.is_empty() {
        0
    } else if text.ends_with(' ') {
        text.len() - 1 // ' '.len_utf8() == 1
    } else {
        last_word_start(text)
    }
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
