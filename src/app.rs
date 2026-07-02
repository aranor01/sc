use std::cell::Cell;
use std::io::stdout;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthChar;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    cursor::{Hide, Show},
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
use crate::ipc::{parse_message, CmdlineInjectMode, IpcMessage, IpcServer};
use crate::history::CommandHistory;
use crate::macros::{MacroContext, PanelContext};
use crate::provider::{NodeKind, NodePath, TreeProvider};
use crate::provider::filesystem::FilesystemProvider;
use crate::state::{AppState, Orientation};
use crate::ui::button::Button;
use crate::ui::button_bar::ButtonBarWidget;
use crate::ui::status_bar::{StatusBarState, StatusBarWidget};
use crate::ui::cmdline::{CmdLineState, CmdLineWidget};
use crate::ui::popup_list::{PopupDirection, PopupListState, PopupListWidget};
use crate::ui::dialog::{render_confirm, render_error, render_input_dialog, CheckboxOptions, ConfirmButtonAreas, ConfirmOp, ConfirmState, ErrorButtonArea, InputDialogAction, InputDialogAreas, InputDialogState};
use crate::ui::menu::{UserMenuAreas, UserMenuState, UserMenuWidget};
use crate::ui::output_overlay::{OutputOverlayState, OutputOverlayWidget};
use crate::ui::modal_event::{CmdlineOutcome, ModalOutcome, OverlayOutcome, PanelOutcome, PopupOutcome};
use crate::ui::panel::{build_filter_pattern, PanelState, PanelWidget, SortKey};

// ── Mode enums ────────────────────────────────────────────────────────────────

pub enum ShellMode {
    Stateless,
    Subshell(crate::subshell::Subshell),
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
            left: absolutize(d1, cwd),
            right: absolutize(right, cwd),
        };
    }
    let restore = flag.unwrap_or(restore_paths_config);
    if restore {
        if let Some((left, right)) = saved {
            return StartupPaths {
                left: absolutize(left, cwd),
                right: absolutize(right, cwd),
            };
        }
    }
    StartupPaths {
        left: cwd.to_path_buf(),
        right: cwd.to_path_buf(),
    }
}

fn absolutize(path: &Path, cwd: &Path) -> PathBuf {
    let joined = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    normalize_lexical(&joined)
}

/// Lexically collapses `.` and `..` components. Deliberately not `fs::canonicalize`: that
/// requires the path to exist (breaks the deleted-directory corner case) and resolves
/// symlinks, which would surprise a user navigating through one.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Abbreviates the home directory prefix as `~` for display; leaves other paths unchanged.
fn display_path(path: &str) -> String {
    abbreviate_home(path, dirs::home_dir().as_deref())
}

fn abbreviate_home(path: &str, home: Option<&Path>) -> String {
    let Some(home) = home else { return path.to_string(); };
    let home = home.to_string_lossy();
    if path == home.as_ref() {
        return "~".to_string();
    }
    if let Some(rest) = path.strip_prefix(home.as_ref()).and_then(|r| r.strip_prefix('/')) {
        return format!("~/{rest}");
    }
    path.to_string()
}

/// Like `absolutize`, but joins against the process cwd for call sites with no `cwd` on hand.
fn absolutize_str(path: &str) -> String {
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        cwd.join(p)
    };
    normalize_lexical(&joined).to_string_lossy().into_owned()
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
    ToggleShellAndSyncCommandLine,
    ToggleCmdline,
    ToggleButtonBar,
    CmdlineHistoryPrev,
    CmdlineHistoryNext,
    ReverseSearch,
    SyncPanels,
    Rename,
    SortPanel,
    Quicksearch,
    ToggleHidden,
    BookmarkOpen,
    BookmarkAdd,
    Mkdir,
    PathHistory,
    Filter,
    SelectGroup,
    UnselectGroup,
    RefreshPanel,
    GoToParent,
    GoBack,
    GoForward,
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


struct ReverseSearchSession {
    list: PopupListState,
}


// ── Modal ─────────────────────────────────────────────────────────────────────

enum Modal {
    None,
    UserMenu(UserMenuState),
    Confirm(ConfirmState),
    Error(String),
    InputDialog(InputDialogState),
    SortPopup(PopupListState, Side),
    BookmarkList(PopupListState),
    PathHistoryList(PopupListState),
}

// ── ModalAreas ────────────────────────────────────────────────────────────────

enum ModalAreas {
    None,
    Confirm(ConfirmButtonAreas),
    UserMenu(UserMenuAreas),
    Error(ErrorButtonArea),
    InputDialog(InputDialogAreas, Option<Position>),
    SortPopup(Rect, usize),
    BookmarkList(Rect, usize),
    PathHistoryList(Rect, usize),
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
    subshell_prompt_needed: bool,
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
    input_ok_btn: Cell<Button>,
    input_cancel_btn: Cell<Button>,
    input_cb_files_only: Cell<Option<Rect>>,
    input_cb_case_sensitive: Cell<Option<Rect>>,
    input_cb_regexp: Cell<Option<Rect>>,
    menu_list_area: Cell<Rect>,
    menu_list_offset: Cell<usize>,
    // Popup list hit-test areas and scroll offsets (reset each frame when not visible)
    completion_popup_area: Cell<Rect>,
    completion_popup_offset: Cell<usize>,
    rev_search_popup_area: Cell<Rect>,
    rev_search_popup_offset: Cell<usize>,
    sort_popup_area: Cell<Rect>,
    sort_popup_offset: Cell<usize>,
    bookmark_popup_area: Cell<Rect>,
    bookmark_popup_offset: Cell<usize>,
    bookmarks: Vec<String>,
    path_history_popup_area: Cell<Rect>,
    path_history_popup_offset: Cell<usize>,
    panel_history_left: crate::panel_history::PanelHistory,
    panel_history_right: crate::panel_history::PanelHistory,
    // Pending left-button press for down+up click detection
    mouse_pressed: Option<Position>,
    should_quit: bool,
    mouse: bool,
    // True when cmdline is empty OR user pressed ESC to give panels focus temporarily.
    // Cleared after the first keystroke (if cmdline is non-empty).
    explicit_action_mode: bool,
    completion: Option<CompletionSession>,
    reverse_search: Option<ReverseSearchSession>,
    quicksearch: Option<String>,
    shell_mode: ShellMode,
    needs_full_redraw: bool,
    ipc: Option<IpcServer>,
    status: StatusBarState,
}

fn shell_escape_path(s: &str) -> String {
    crate::macros::shell_escape(s)
}

/// Parse an IPC pattern: strip the legacy `/` prefix for regex, return (text, is_regexp).
fn ipc_pattern(pattern: &str) -> (&str, bool) {
    if let Some(rest) = pattern.strip_prefix('/') {
        (rest, true)
    } else {
        (pattern, false)
    }
}


// OSC alternative is first so \x1b] is consumed with its body;
// otherwise the fallback 2-char match would consume just \x1b].
// [0-?] adds DEC private sequences (\x1b=, \x1b>, \x1b7, \x1b8, etc.) missing from the
// original [@-Z\\-_] range (0x40-0x5F).
static ANSI_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\x1b(?:\][^\x07\x1b]*(?:\x07|\x1b\\)|[0-?@-Z\\-_]|\[[0-?]*[ -/]*[@-~])",
    )
    .unwrap()
});

fn normalize_pty_output(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    // PTY converts \n → \r\n; remove the bare \r.
    let s = s.replace('\r', "");
    let s = ANSI_RE.replace_all(&s, "");
    let tab_count = s.bytes().filter(|&b| b == b'\t').count();
    let mut out = String::with_capacity(s.len() + tab_count * 7);
    let mut col: usize = 0;
    const SPACES: &str = "        "; // 8 spaces — max tab expansion
    for ch in s.chars() {
        match ch {
            '\t' => {
                let spaces = 8 - (col % 8);
                out.push_str(&SPACES[..spaces]);
                col += spaces;
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            c if (c as u32) < 0x20 || c == '\x7f' => {}
            _ => {
                out.push(ch);
                col += ch.width().unwrap_or(1);
            }
        }
    }
    out
}



impl App {
    pub fn new(config: Config, left_path: PathBuf, right_path: PathBuf, state: &AppState,
               mut ph_left: crate::panel_history::PanelHistory,
               mut ph_right: crate::panel_history::PanelHistory,
               mouse: bool) -> Self {
        let left_node = NodePath(left_path.to_string_lossy().into_owned());
        let right_node = NodePath(right_path.to_string_lossy().into_owned());
        if ph_left.current_path() != Some(left_node.0.as_str()) { ph_left.push(&left_node.0); }
        if ph_right.current_path() != Some(right_node.0.as_str()) { ph_right.push(&right_node.0); }

        let history = crate::state::history_path();
        let hist = if history.exists() {
            CommandHistory::load(&history).unwrap_or_else(|_| CommandHistory::new())
        } else {
            CommandHistory::new()
        };

        let mut app = App {
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
            subshell_prompt_needed: false,
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
            input_ok_btn: Cell::new(Button::default()),
            input_cancel_btn: Cell::new(Button::default()),
            input_cb_files_only: Cell::new(None),
            input_cb_case_sensitive: Cell::new(None),
            input_cb_regexp: Cell::new(None),
            menu_list_area: Cell::new(Rect::default()),
            menu_list_offset: Cell::new(0),
            completion_popup_area: Cell::new(Rect::default()),
            completion_popup_offset: Cell::new(0),
            rev_search_popup_area: Cell::new(Rect::default()),
            rev_search_popup_offset: Cell::new(0),
            sort_popup_area: Cell::new(Rect::default()),
            sort_popup_offset: Cell::new(0),
            bookmark_popup_area: Cell::new(Rect::default()),
            bookmark_popup_offset: Cell::new(0),
            bookmarks: crate::bookmarks::load(),
            path_history_popup_area: Cell::new(Rect::default()),
            path_history_popup_offset: Cell::new(0),
            panel_history_left: ph_left,
            panel_history_right: ph_right,
            mouse_pressed: None,
            should_quit: false,
            mouse,
            explicit_action_mode: false,
            completion: None,
            reverse_search: None,
            quicksearch: None,
            shell_mode: ShellMode::Stateless,
            needs_full_redraw: false,
            ipc: IpcServer::new(),
            status: StatusBarState::default(),
            config,
        };
        // Try to spawn subshell if configured
        if app.config.startup.subshell {
            // Err falls back to stateless silently.
            if let Ok(sub) = crate::subshell::Subshell::spawn() {
                app.shell_mode = ShellMode::Subshell(sub);
            }
        }
        // Restore saved sort and hidden state, then re-sort
        app.left.sort_key = state.left_sort_key;
        app.left.sort_asc = state.left_sort_asc;
        app.left.show_hidden = state.left_show_hidden;
        app.right.sort_key = state.right_sort_key;
        app.right.sort_asc = state.right_sort_asc;
        app.right.show_hidden = state.right_show_hidden;
        app.left.refresh();
        app.right.refresh();
        app
    }

    fn action_mode(&self) -> bool {
        self.cmdline.is_empty() || self.explicit_action_mode || self.pending_chord.is_some()
    }

    fn set_status(&mut self, text: &str, warn: bool) {
        self.status.set(text, warn);
    }

    fn status_active(&self) -> bool {
        self.status.is_active()
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
        // 2 borders + 1 header row = 3 overhead rows
        area.height.saturating_sub(3).max(1) as usize
    }

    fn active_vh(&self) -> usize {
        self.panel_visible_height(self.active)
    }

    fn bindings_list(&self) -> Vec<(&ActionBindings, Action)> {
        let kb = &self.config.keybindings;
        vec![
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
            (&kb.toggle_shell_and_sync_command_line, Action::ToggleShellAndSyncCommandLine),
            (&kb.toggle_cmdline, Action::ToggleCmdline),
            (&kb.toggle_button_bar, Action::ToggleButtonBar),
            (&kb.cmdline_history_prev, Action::CmdlineHistoryPrev),
            (&kb.cmdline_history_next, Action::CmdlineHistoryNext),
            (&kb.reverse_search, Action::ReverseSearch),
            (&kb.sync_panels, Action::SyncPanels),
            (&kb.rename, Action::Rename),
            (&kb.sort_panel, Action::SortPanel),
            (&kb.quicksearch, Action::Quicksearch),
            (&kb.toggle_hidden, Action::ToggleHidden),
            (&kb.bookmark_open, Action::BookmarkOpen),
            (&kb.bookmark_add, Action::BookmarkAdd),
            (&kb.mkdir, Action::Mkdir),
            (&kb.path_history, Action::PathHistory),
            (&kb.filter, Action::Filter),
            (&kb.select_group, Action::SelectGroup),
            (&kb.unselect_group, Action::UnselectGroup),
            (&kb.refresh_panel, Action::RefreshPanel),
            (&kb.go_to_parent, Action::GoToParent),
            (&kb.go_back, Action::GoBack),
            (&kb.go_forward, Action::GoForward),
        ]
    }

    fn sort_popup_items() -> Vec<String> {
        vec![
            "Name ▲".to_string(), "Name ▼".to_string(),
            "Extension ▲".to_string(), "Extension ▼".to_string(),
            "Size ▲".to_string(), "Size ▼".to_string(),
            "Modified ▲".to_string(), "Modified ▼".to_string(),
            "Unsorted".to_string(),
        ]
    }

    fn sort_item_index(key: SortKey, asc: bool) -> usize {
        match (key, asc) {
            (SortKey::Name, true) => 0, (SortKey::Name, false) => 1,
            (SortKey::Extension, true) => 2, (SortKey::Extension, false) => 3,
            (SortKey::Size, true) => 4, (SortKey::Size, false) => 5,
            (SortKey::Modified, true) => 6, (SortKey::Modified, false) => 7,
            _ => 8,
        }
    }

    fn sort_item_to_key(idx: usize) -> (SortKey, bool) {
        match idx {
            0 => (SortKey::Name, true), 1 => (SortKey::Name, false),
            2 => (SortKey::Extension, true), 3 => (SortKey::Extension, false),
            4 => (SortKey::Size, true), 5 => (SortKey::Size, false),
            6 => (SortKey::Modified, true), 7 => (SortKey::Modified, false),
            _ => (SortKey::Unsorted, true),
        }
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
                self.modal = Modal::UserMenu(UserMenuState::new(self.config.menu.clone()));
            }
            Action::TagFile => {
                let vh = self.active_vh();
                self.active_panel_mut().tag_toggle(vh, true);
            }
            Action::InvertTags => {
                self.active_panel_mut().invert_tags();
            }
            Action::Copy => {
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
            Action::Rename => {
                let name = self.active_panel().current_name();
                if !name.is_empty() && name != ".." {
                    let state = InputDialogState::new(InputDialogAction::Rename, " Rename ", &name);
                    self.modal = Modal::InputDialog(state);
                }
            }
            Action::SortPanel => {
                let panel = self.active_panel();
                let selected = Self::sort_item_index(panel.sort_key, panel.sort_asc);
                let popup = PopupListState { items: Self::sort_popup_items(), selected };
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
                let state = InputDialogState::new(InputDialogAction::Mkdir, " Create directory ", "");
                self.modal = Modal::InputDialog(state);
            }
            Action::PathHistory => {
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
                self.active_panel_mut().refresh();
            }
            Action::GoToParent => {
                let current = self.active_panel().path.clone();
                let parent_opt = self.active_panel().provider.parent(&current);
                let Some(parent_path) = parent_opt else { return; };
                let dest = parent_path.0.clone();
                let panel = self.active_panel_mut();
                panel.path = parent_path;
                panel.cursor = 0;
                panel.scroll = 0;
                panel.tagged.clear();
                panel.refresh();
                self.push_path_history(&dest);
            }
            Action::GoBack => {
                let path_opt = match self.active {
                    Side::Left => self.panel_history_left.go_back(),
                    Side::Right => self.panel_history_right.go_back(),
                };
                let Some(path) = path_opt else { return; };
                let path = absolutize_str(&path);
                if !std::path::Path::new(&path).exists() {
                    match self.active {
                        Side::Left => { self.panel_history_left.go_forward(); }
                        Side::Right => { self.panel_history_right.go_forward(); }
                    }
                    self.set_status(&format!("Path no longer exists: {path}"), true);
                    return;
                }
                let panel = self.active_panel_mut();
                panel.path = crate::provider::NodePath(path);
                panel.cursor = 0;
                panel.scroll = 0;
                panel.tagged.clear();
                panel.refresh();
            }
            Action::GoForward => {
                let path_opt = match self.active {
                    Side::Left => self.panel_history_left.go_forward(),
                    Side::Right => self.panel_history_right.go_forward(),
                };
                let Some(path) = path_opt else { return; };
                let path = absolutize_str(&path);
                if !std::path::Path::new(&path).exists() {
                    match self.active {
                        Side::Left => { self.panel_history_left.go_back(); }
                        Side::Right => { self.panel_history_right.go_back(); }
                    }
                    self.set_status(&format!("Path no longer exists: {path}"), true);
                    return;
                }
                let panel = self.active_panel_mut();
                panel.path = crate::provider::NodePath(path);
                panel.cursor = 0;
                panel.scroll = 0;
                panel.tagged.clear();
                panel.refresh();
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
        }
    }

    fn push_path_history(&mut self, path: &str) {
        match self.active {
            Side::Left => self.panel_history_left.push(path),
            Side::Right => self.panel_history_right.push(path),
        }
    }

    fn navigate_to_path(&mut self, path: &str) {
        let path = absolutize_str(path);
        if !std::path::Path::new(&path).exists() {
            self.modal = Modal::Error(format!("Path no longer exists: {path}"));
            return;
        }
        let panel = self.active_panel_mut();
        panel.path = crate::provider::NodePath(path.clone());
        panel.cursor = 0;
        panel.scroll = 0;
        panel.tagged.clear();
        panel.refresh();
        self.push_path_history(&path);
    }

    fn navigate_to_bookmark(&mut self, path: &str) {
        let path = absolutize_str(path);
        if !std::path::Path::new(&path).exists() {
            self.bookmarks.retain(|b| b != &path);
            let _ = crate::bookmarks::save(&self.bookmarks);
            self.modal = Modal::Error(format!("Path no longer exists: {path}"));
            return;
        }
        let panel = self.active_panel_mut();
        panel.path = crate::provider::NodePath(path.clone());
        panel.cursor = 0;
        panel.scroll = 0;
        panel.tagged.clear();
        panel.refresh();
        self.push_path_history(&path);
    }

    fn quicksearch_jump(&mut self, pattern: &str) {
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

    fn reopen_dialog_with_error(&mut self, mut state: InputDialogState, err: String) {
        state.error = Some(err);
        self.modal = Modal::InputDialog(state);
    }

    fn execute_input_dialog(&mut self, state: InputDialogState) {
        let new_text = state.input.text.trim().to_string();
        match state.action {
            InputDialogAction::Rename => {
                if new_text.is_empty() { return; }
                let current = self.active_panel().current_name();
                if current.is_empty() || current == ".." { return; }
                let result = {
                    let panel = self.active_panel();
                    let src = panel.provider.join(&panel.path, &current);
                    panel.provider.rename(&src, &new_text)
                };
                match result {
                    Ok(()) => {
                        self.active_panel_mut().refresh();
                        let idx = self.active_panel().entries.iter()
                            .position(|e| e.name == new_text)
                            .unwrap_or(0);
                        let vh = self.active_vh();
                        let panel = self.active_panel_mut();
                        panel.cursor = idx.min(panel.entries.len().saturating_sub(1));
                        let scroll = panel.scroll;
                        if panel.cursor < scroll {
                            panel.scroll = panel.cursor;
                        } else if panel.cursor >= scroll + vh.max(1) {
                            panel.scroll = panel.cursor + 1 - vh.max(1);
                        }
                    }
                    Err(e) => {
                        self.modal = Modal::Error(e.to_string());
                    }
                }
            }
            InputDialogAction::Mkdir => {
                if new_text.is_empty() { return; }
                let result = {
                    let panel = self.active_panel();
                    panel.provider.mkdir(&panel.path, &new_text)
                };
                match result {
                    Ok(()) => {
                        self.active_panel_mut().refresh();
                        let idx = self.active_panel().entries.iter()
                            .position(|e| e.name == new_text)
                            .unwrap_or(0);
                        let vh = self.active_vh();
                        let panel = self.active_panel_mut();
                        panel.cursor = idx.min(panel.entries.len().saturating_sub(1));
                        let scroll = panel.scroll;
                        if panel.cursor < scroll {
                            panel.scroll = panel.cursor;
                        } else if panel.cursor >= scroll + vh.max(1) {
                            panel.scroll = panel.cursor + 1 - vh.max(1);
                        }
                    }
                    Err(e) => {
                        self.modal = Modal::Error(e.to_string());
                    }
                }
            }
            InputDialogAction::Filter => {
                let opts = state.checkboxes.as_ref().cloned().unwrap_or_default();
                if new_text.is_empty() {
                    self.active_panel_mut().filter = None;
                    self.active_panel_mut().refresh();
                } else {
                    match build_filter_pattern(&new_text, opts.files_only, opts.case_sensitive, opts.is_regexp) {
                        Ok(pat) => {
                            self.active_panel_mut().filter = Some(pat);
                            self.active_panel_mut().refresh();
                        }
                        Err(e) => {
                            self.reopen_dialog_with_error(state, e);
                        }
                    }
                }
            }
            InputDialogAction::SelectGroup => {
                if new_text.is_empty() { return; }
                let opts = state.checkboxes.as_ref().cloned().unwrap_or_default();
                match build_filter_pattern(&new_text, opts.files_only, opts.case_sensitive, opts.is_regexp) {
                    Ok(pat) => {
                        let names: Vec<String> = self.active_panel().entries.iter()
                            .filter(|e| {
                                e.name != ".."
                                    && !(opts.files_only && e.kind == NodeKind::Dir)
                                    && pat.matches(&e.name)
                            })
                            .map(|e| e.name.clone())
                            .collect();
                        let panel = self.active_panel_mut();
                        for name in names { panel.tagged.insert(name); }
                    }
                    Err(e) => {
                        self.reopen_dialog_with_error(state, e);
                    }
                }
            }
            InputDialogAction::UnselectGroup => {
                if new_text.is_empty() { return; }
                let opts = state.checkboxes.as_ref().cloned().unwrap_or_default();
                // Collect directory names before the mutable borrow
                let dir_names: std::collections::HashSet<String> = if opts.files_only {
                    self.active_panel().entries.iter()
                        .filter(|e| e.kind == NodeKind::Dir)
                        .map(|e| e.name.clone())
                        .collect()
                } else {
                    std::collections::HashSet::new()
                };
                match build_filter_pattern(&new_text, opts.files_only, opts.case_sensitive, opts.is_regexp) {
                    Ok(pat) => {
                        let panel = self.active_panel_mut();
                        panel.tagged.retain(|n| {
                            // If files_only and this is a directory, keep it tagged
                            if pat.files_only && dir_names.contains(n) { return true; }
                            !pat.matches(n)
                        });
                    }
                    Err(e) => {
                        self.reopen_dialog_with_error(state, e);
                    }
                }
            }
        }
    }

    /// Call the completion script with the current cmdline text.
    /// Returns `(candidates, warning)` where `warning` is set when the script is
    /// missing or exits non-zero with no output.
    fn collect_candidates(&self) -> (Vec<String>, Option<String>) {
        if self.cmdline.text.trim().is_empty() {
            return (vec![], None);
        }
        let Some(script) = find_complete_script() else {
            return (vec![], Some("Completion script not found".to_string()));
        };
        match std::process::Command::new("bash")
            .arg(&script)
            .arg(&self.cmdline.text)
            .current_dir(&self.active_panel().path.0)
            .output()
        {
            Ok(out) => {
                let candidates: Vec<String> = std::str::from_utf8(&out.stdout)
                    .unwrap_or("")
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                if !out.status.success() && candidates.is_empty() {
                    (vec![], Some("Completion script failed".to_string()))
                } else {
                    (candidates, None)
                }
            }
            Err(_) => (vec![], Some("Completion script not found".to_string())),
        }
    }

    /// Replace the last word in the cmdline with `candidate`.
    fn apply_word_replacement(&mut self, word_start: usize, candidate: &str) {
        self.cmdline.text.truncate(word_start);
        self.cmdline.cursor = word_start;
        self.cmdline.insert_str(candidate);
        if !candidate.ends_with(' ') || candidate.ends_with("\\ ") {
            self.cmdline.insert_str(" ");
        }
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
        let (candidates, warn) = self.collect_candidates();
        if let Some(msg) = warn {
            self.set_status(&msg, true);
            self.completion = None;
            return;
        }
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
    fn update_reverse_search(&mut self) {
        let Some(session) = &mut self.reverse_search else { return };
        let new_items = history_matches(&self.history, &self.cmdline.text);
        session.list.selected = new_items.len().saturating_sub(1);
        session.list.items = new_items;
        self.rev_search_popup_offset.set(0);
    }

    fn execute_menu_item(&mut self, cmd_template: String) {
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
        self.execute_command(false, prior_cmdline);
    }

    fn execute_command(&mut self, push_to_history: bool, restore_to: CmdLineState) {
        let cmd = self.cmdline.text.clone();
        if cmd.is_empty() {
            return;
        }
        if push_to_history {
            self.history.push(cmd.clone());
            let _ = self.history.save(&crate::state::history_path());
        }
        self.cmdline = restore_to;

        let cwd = self.active_panel().path.0.clone();

        // Tear down TUI so any spawned process gets a real terminal.
        if self.mouse {
            let _ = crossterm::execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture, Show);
        } else {
            let _ = crossterm::execute!(stdout(), LeaveAlternateScreen, Show);
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
                let _ = std::io::Write::write_all(
                    &mut std::io::stdout(),
                    format!("[sc]$ cd {}\r\n", shell_escape_path(&cwd)).as_bytes(),
                );
                let _ = sub.send_line(&format!(" cd {}", shell_escape_path(&cwd)));
            }
        }

        // Echo the command so the user sees what ran and in what order.
        let _ = std::io::Write::write_all(
            &mut std::io::stdout(),
            format!("[sc]$ {}\r\n", cmd).as_bytes(),
        );

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
        self.last_output = if text.trim().is_empty() { None } else { Some(text) };

        let _ = enable_raw_mode();
        if self.mouse {
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen, EnableMouseCapture);
        } else {
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen);
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

    fn handle_key_event_inner(&mut self, event: KeyEvent) {
        // Output overlay active — block all input except scroll/dismiss keys
        if self.show_output {
            match self.overlay.handle_key(&event, &self.config.keybindings.toggle_shell) {
                OverlayOutcome::Dismissed => { self.show_output = false; return; }
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
                | KeyCode::Home | KeyCode::End => {
                    self.quicksearch = None;
                    let am = self.action_mode();
                    let vh = self.active_vh();
                    self.active_panel_mut().handle_key(&event, vh, am);
                }
                KeyCode::Enter | KeyCode::Esc => {
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
            PanelOutcome::ExecuteCommand => { self.execute_command(true, CmdLineState::new()); return; }
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
                            self.navigate_to_path(&p);
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
            Modal::UserMenu(_) => {
                if close_btn.clicked(down, up) {
                    self.modal = Modal::None;
                } else if let Some(cmd_template) = menu_item_cmd {
                    self.modal = Modal::None;
                    self.execute_menu_item(cmd_template);
                }
            }
        }
    }

    fn handle_button_bar_click(&mut self, pos: Position) {
        let bb_area = self.button_bar_area.get();
        if let Some(n) = ButtonBarWidget::button_at(&self.config.keybindings, &self.config.menu, bb_area, pos) {
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

        let inner_y = clicked_area.y + 1;  // first row inside border = header row
        let entries_y = inner_y + 1;       // entries start below header
        if row < inner_y || row >= clicked_area.y + clicked_area.height - 1 {
            return;
        }
        // Header row click: sort by column
        if row == inner_y {
            if clicked_side != self.active {
                self.active = clicked_side;
            }
            let inner_x = clicked_area.x + 1;
            let inner_width = clicked_area.width.saturating_sub(2) as usize;
            let fixed = 22usize; // same formula as panel.rs
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
            } else if rel_x > 2 + name_width && rel_x < 2 + name_width + 9 {
                // Size column
                if panel.sort_key == SortKey::Size {
                    panel.sort_asc = !panel.sort_asc;
                } else {
                    panel.sort_key = SortKey::Size;
                    panel.sort_asc = true;
                }
                panel.refresh();
            } else if rel_x >= 2 + name_width + 10 && rel_x < 2 + name_width + 20 {
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
                if clicked_side != self.active {
                    self.active = clicked_side;
                }
                let nav_err = {
                    let panel = match clicked_side {
                        Side::Left => &mut self.left,
                        Side::Right => &mut self.right,
                    };
                    panel.move_cursor_to_row(entry_row, vh);
                    if is_double && panel.current_entry().map(|e| e.kind == NodeKind::Dir).unwrap_or(false) {
                        panel.enter_dir()
                    } else {
                        None
                    }
                };
                if let Some(err) = nav_err {
                    self.set_status(&err, true);
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
                panel.tag_toggle(vh, false);
            }
            MouseButton::Middle => {}
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        let pos = Position { x: col, y: row };

        // Output overlay — capture all mouse input; only scroll/scrollbar clicks are meaningful.
        if self.show_output {
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
                        let total_lines = self.last_output.as_deref()
                            .map(|t| t.lines().count())
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
                        Modal::UserMenu(s) => s.move_up(),
                        Modal::SortPopup(s, _) => s.move_up(),
                        Modal::BookmarkList(s) => s.move_up(),
                        Modal::PathHistoryList(s) => s.move_up(),
                        _ => {}
                    }
                }
                MouseEventKind::ScrollDown => {
                    match &mut self.modal {
                        Modal::UserMenu(s) => s.move_down(),
                        Modal::SortPopup(s, _) => s.move_down(),
                        Modal::BookmarkList(s) => s.move_down(),
                        Modal::PathHistoryList(s) => s.move_down(),
                        _ => {}
                    }
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

    fn run_subshell_passthrough(&mut self, sync_cmdline: bool) {
        // Leave alternate screen so the subshell renders in the normal terminal
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
            let cwd = self.active_panel().path.0.clone();
            // Discard any buffered readline echoes from `history -s` calls that
            // accumulated since the last passthrough session.
            sub.drain();
            // Sync PTY window size so fullscreen programs (vim, mc) get the real dimensions.
            if let Ok((cols, rows)) = crossterm::terminal::size() {
                sub.resize(cols, rows);
            }
            // Only sync cwd if the subshell is not already in the panel's directory.
            let shell_cwd = std::fs::read_link(format!("/proc/{}/cwd", sub.child_pid))
                .ok()
                .map(|p| p.to_string_lossy().to_string());
            if shell_cwd.as_deref() != Some(cwd.as_str()) {
                // \x15 (Ctrl+U) kills any readline-buffered partial input before we
                // inject the cd, preventing accidental execution of user-typed text.
                let cd_cmd = format!("\x15 cd {}", shell_escape_path(&cwd));
                let _ = sub.send_line(&cd_cmd);
                // cd echo + post-cd prompt will be visible at session start
                if !cmdline_text.is_empty() {
                    sub.send_raw(cmdline_text.as_bytes());
                }
            } else if self.subshell_prompt_needed {
                // A sc cmdline command ran since last passthrough: drain() discarded
                // bash's PS1, so we need \n to force a fresh one. \x15 clears any
                // partial readline input first to prevent accidental execution.
                self.subshell_prompt_needed = false;
                let _ = sub.send_line("\x15");
                if !cmdline_text.is_empty() {
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
            } else {
                // Sync active panel to wherever the subshell ended up.
                if let Ok(link) = std::fs::read_link(format!("/proc/{}/cwd", sub.child_pid)) {
                    let new_cwd = link.to_string_lossy().to_string();
                    let panel_cwd = self.active_panel().path.0.clone();
                    if new_cwd != panel_cwd {
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
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen, EnableMouseCapture);
        } else {
            let _ = crossterm::execute!(stdout(), Hide, EnterAlternateScreen);
        }
        // Refresh panels in case the subshell changed the filesystem
        self.needs_full_redraw = true;
        self.left.refresh();
        self.right.refresh();
    }

    fn save_state(&self) {
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
        };
        let _ = state.save();
        let _ = self.history.save(&crate::state::history_path());
        let _ = crate::panel_history::save(&self.panel_history_left, &self.panel_history_right);
    }

    fn render(&mut self, frame: &mut Frame) {
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
        let left_active = self.active == Side::Left;
        let left_title = display_path(&self.left.path.0);
        frame.render_stateful_widget(
            PanelWidget { cs: &cs, active: left_active, title: left_title },
            layout.left,
            &mut self.left,
        );
        let right_active = self.active == Side::Right;
        let right_title = display_path(&self.right.path.0);
        frame.render_stateful_widget(
            PanelWidget { cs: &cs, active: right_active, title: right_title },
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
                if matches!(self.modal, Modal::None) && !self.show_output && command_line_is_active {
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
                        let anchor_byte = word_anchor_byte(&self.cmdline.text);
                        let anchor_chars = self.cmdline.text[..anchor_byte].chars().count();
                        let total_col = prompt_len + anchor_chars;
                        let anchor_x = cmdline_area.x + (total_col % width) as u16;
                        let anchor_y = cmdline_area.y + (total_col / width) as u16;
                        let (r, offset) = PopupListWidget { cs: &cs, state: &session.list, title: None, direction: PopupDirection::Above }
                            .render_at(area, frame.buffer_mut(), anchor_x, anchor_y, self.completion_popup_offset.get());
                        self.completion_popup_area.set(r);
                        self.completion_popup_offset.set(offset);
                    }
                } else {
                    self.completion_popup_area.set(Rect::default());
                    self.completion_popup_offset.set(0);
                }

                if let Some(session) = self.reverse_search.as_ref() {
                    let (r, offset) = PopupListWidget { cs: &cs, state: &session.list, title: None, direction: PopupDirection::Above }
                        .render_at(area, frame.buffer_mut(), cmdline_area.x, cmdline_area.y, self.rev_search_popup_offset.get());
                    self.rev_search_popup_area.set(r);
                    self.rev_search_popup_offset.set(offset);
                } else {
                    self.rev_search_popup_area.set(Rect::default());
                    self.rev_search_popup_offset.set(0);
                }
            }
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

        // Output overlay — drawn after cmdline/button bar so it covers the full terminal area
        if self.show_output {
            if let Some(text) = &self.last_output {
                self.overlay_area.set(area);
                let overlay = OutputOverlayWidget {
                    cs: &cs,
                    text,
                    scroll: self.overlay.scroll,
                };
                frame.render_widget(overlay, area);
            }
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
            Modal::InputDialog(state) => {
                let (a, cursor) = render_input_dialog(area, frame.buffer_mut(), &cs, state, press);
                ModalAreas::InputDialog(a, cursor)
            }
            Modal::SortPopup(state, side) => {
                let panel_area = match side {
                    Side::Left => self.left_area.get(),
                    Side::Right => self.right_area.get(),
                };
                let anchor_x = panel_area.x + 2;
                let anchor_y = panel_area.y + 1;
                let offset = self.sort_popup_offset.get();
                let (r, new_offset) = PopupListWidget { cs: &cs, state, title: Some("Sort by"), direction: PopupDirection::Below }
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
                let (r, new_offset) = PopupListWidget { cs: &cs, state, title: Some("Path History"), direction: PopupDirection::Below }
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
                let (r, new_offset) = PopupListWidget { cs: &cs, state, title: Some("Directory Bookmarks"), direction: PopupDirection::Below }
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
            ModalAreas::UserMenu(a) => {
                self.menu_list_area.set(a.list_area);
                self.menu_list_offset.set(a.list_offset);
                self.menu_close_btn.set(a.close);
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

    fn handle_ipc(&mut self, msg: IpcMessage) {
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
                    let panel_cwd = self.active_panel().path.0.clone();
                    if new_cwd != panel_cwd {
                        self.navigate_to_path(&new_cwd);
                    }
                }
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
                    }
                    _ => {}
                }
            }

            if let Some(msg) = self.ipc.as_ref().and_then(|ipc| ipc.try_recv()) {
                self.handle_ipc(msg);
            }
        }

        self.save_state();
        Ok(())
    }
}

fn find_complete_script() -> Option<PathBuf> {
    let dir = crate::config::find_scripts_dir()?;
    for name in &["sc-complete", "complete.sh"] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
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

    #[test]
    fn dir1_relative_is_absolutized_against_cwd() {
        let r = rp(Some("subdir"), None, None, false, None, "/cwd");
        assert_eq!(r, StartupPaths { left: p("/cwd/subdir"), right: p("/cwd/subdir") });
    }

    #[test]
    fn saved_relative_is_absolutized_against_cwd() {
        let r = rp(None, None, Some(true), false, Some(("rel/left", "/abs/right")), "/cwd");
        assert_eq!(r, StartupPaths { left: p("/cwd/rel/left"), right: p("/abs/right") });
    }

    #[test]
    fn dir1_with_dotdot_is_normalized() {
        let r = rp(Some("../../dir"), None, None, false, None, "/cwd/a/b");
        assert_eq!(r, StartupPaths { left: p("/cwd/dir"), right: p("/cwd/dir") });
    }

    #[test]
    fn dir1_with_trailing_dot_is_normalized() {
        let r = rp(Some("."), None, None, false, None, "/cwd/a");
        assert_eq!(r, StartupPaths { left: p("/cwd/a"), right: p("/cwd/a") });
    }

    #[test]
    fn normalize_lexical_collapses_dotdot() {
        let r = normalize_lexical(Path::new("/tmp/a/b/../../dir"));
        assert_eq!(r, p("/tmp/dir"));
    }

    #[test]
    fn normalize_lexical_collapses_trailing_dot() {
        let r = normalize_lexical(Path::new("/tmp/a/b/."));
        assert_eq!(r, p("/tmp/a/b"));
    }

    #[test]
    fn normalize_lexical_dotdot_above_root_stays_at_root() {
        let r = normalize_lexical(Path::new("/../../foo"));
        assert_eq!(r, p("/foo"));
    }

    #[test]
    fn normalize_lexical_no_change_for_clean_path() {
        let r = normalize_lexical(Path::new("/tmp/a/b"));
        assert_eq!(r, p("/tmp/a/b"));
    }

    #[test]
    fn absolutize_str_normalizes_absolute_input_with_dotdot() {
        let r = absolutize_str("/tmp/a/b/../../dir");
        assert_eq!(r, "/tmp/dir");
    }

    #[test]
    fn abbreviate_home_exact_match() {
        let r = abbreviate_home("/home/alice", Some(Path::new("/home/alice")));
        assert_eq!(r, "~");
    }

    #[test]
    fn abbreviate_home_subdir() {
        let r = abbreviate_home("/home/alice/projects", Some(Path::new("/home/alice")));
        assert_eq!(r, "~/projects");
    }

    #[test]
    fn abbreviate_home_respects_segment_boundary() {
        let r = abbreviate_home("/home/alice2/projects", Some(Path::new("/home/alice")));
        assert_eq!(r, "/home/alice2/projects");
    }

    #[test]
    fn abbreviate_home_unrelated_path_unchanged() {
        let r = abbreviate_home("/var/log", Some(Path::new("/home/alice")));
        assert_eq!(r, "/var/log");
    }

    #[test]
    fn abbreviate_home_no_home_dir_unchanged() {
        let r = abbreviate_home("/var/log", None);
        assert_eq!(r, "/var/log");
    }
}
