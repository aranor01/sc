use std::cell::Cell;
use std::io::stdout;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthChar;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
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

use crate::config::{format_key_spelled, ActionBindings, Config, KeyBinding};
use crate::ipc::{parse_message, CmdlineInjectMode, IpcMessage, IpcServer};
use crate::history::CommandHistory;
use crate::macros::{MacroContext, PanelContext};
use crate::provider::{NodeEntry, NodeKind, NodePath, SearchEvent, SearchHandle, SearchQuery, TreeProvider};
use crate::provider::filesystem::FilesystemProvider;
use crate::state::{AppState, Orientation};
use crate::ui::button::Button;
use crate::ui::button_bar::{BarAction, ButtonBarWidget};
use crate::ui::status_bar::{StatusBarState, StatusBarWidget};
use crate::ui::cmdline::{CmdLineState, CmdLineWidget};
use crate::ui::popup_list::{PopupDirection, PopupListState, PopupListWidget};
use crate::ui::dialog::{render_confirm, render_error, render_input_dialog, render_search_dialog, CheckboxOptions, ConfirmButtonAreas, ConfirmOp, ConfirmState, ErrorButtonArea, InputDialogAction, InputDialogAreas, InputDialogState, SearchDialogAreas, SearchDialogState, SEARCH_CB_FOCUS, SEARCH_INPUT_FOCUS};
use crate::ui::output_overlay::{OutputOverlayState, OutputOverlayWidget};
use crate::ui::modal_event::{CmdlineOutcome, ModalOutcome, OverlayOutcome, PanelOutcome, PopupOutcome};
use crate::pattern::ContentMatcher;
use crate::ui::panel::{build_filter_pattern, MatchesState, PanelContent, PanelState, PanelWidget, SearchResultsState, SortKey};

// ── Mode enums ────────────────────────────────────────────────────────────────

pub enum ShellMode {
    Stateless,
    Subshell(crate::subshell::Subshell),
}

pub enum AppMode {
    Ui,
}

mod startup;
mod actions;
mod commands;
mod completion;
mod dialogs;
mod ipc;
mod keys;
mod mouse;
mod nav;
mod palette;
mod render;
mod search;
mod viewer;
pub use startup::resolve_startup_paths;
#[cfg(test)]
use startup::{abbreviate_home, normalize_lexical, StartupPaths};
use startup::{absolutize_str, display_path};
use commands::{menu_item_is_chord_start, menu_item_matches_key};
use completion::history_matches;
use viewer::{normalize_pty_output, shell_escape_path};
use crate::textbuf::TextBuffer;

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
    Search,
    SelectGroup,
    UnselectGroup,
    RefreshPanel,
    GoToParent,
    GoBack,
    GoForward,
    ToggleMatchesPanel,
    View,
    ActionPalette,
}

// ── KeyMatch ──────────────────────────────────────────────────────────────────

enum KeyMatch {
    Act(Action),
    ChordStart,
    None,
}

fn match_key(
    bindings_list: &[(&'static str, &ActionBindings, Action)],
    event: &KeyEvent,
    pending: Option<&KeyEvent>,
) -> KeyMatch {
    if let Some(first) = pending {
        for (_, bindings, action) in bindings_list {
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
    for (_, bindings, action) in bindings_list {
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

#[derive(Clone, PartialEq, Eq)]
enum PaletteView {
    /// Recent actions and the category list; typing searches every command.
    Root,
    /// The commands of one category.
    Category(String),
    /// Only the user-configured menu commands (F2).
    UserOnly,
}

#[derive(Clone)]
enum PaletteTarget {
    /// Placeholder row ("No matching actions").
    Nothing,
    Category(String),
    Builtin(Action),
    Menu(String),
}

struct PaletteRow {
    /// Recent-list id: the config key of a built-in, or `user:<label>` for a menu entry.
    id: String,
    target: PaletteTarget,
}

struct PaletteSession {
    list: PopupListState,
    rows: Vec<PaletteRow>,
    query: String,
    view: PaletteView,
}


struct PaletteEntry {
    id: String,
    category: String,
    label: String,
    description: String,
    /// Shown after the label: the description cut at its first `(`, `;` or `.`.
    summary: String,
    hint: String,
    target: PaletteTarget,
}

/// The first binding of an action as shown in the palette's shortcut column.
impl PaletteEntry {
    /// Label followed by its short description, as listed in the palette.
    fn display(&self) -> String {
        if self.summary.is_empty() {
            self.label.clone()
        } else {
            format!("{} \u{2013} {}", self.label, self.summary)
        }
    }
}

fn binding_hint(bindings: &ActionBindings) -> String {
    match bindings.first() {
        Some(KeyBinding::Single(k)) => format_key_spelled(k),
        Some(KeyBinding::Chord(a, b)) => format!("{} {}", format_key_spelled(a), format_key_spelled(b)),
        None => String::new(),
    }
}

/// Width of the command palette: the terminal width, capped at this many columns.
const PALETTE_MAX_WIDTH: u16 = 100;

// ── Modal ─────────────────────────────────────────────────────────────────────

enum Modal {
    None,
    Confirm(ConfirmState),
    Error(String),
    InputDialog(InputDialogState),
    SearchDialog(SearchDialogState),
    SortPopup(PopupListState, Side),
    BookmarkList(PopupListState),
    PathHistoryList(PopupListState),
}

// ── ModalAreas ────────────────────────────────────────────────────────────────

enum ModalAreas {
    None,
    Confirm(ConfirmButtonAreas),
    Error(ErrorButtonArea),
    InputDialog(InputDialogAreas, Option<Position>),
    SearchDialog(SearchDialogAreas, Option<Position>),
    SortPopup(Rect, usize),
    BookmarkList(Rect, usize),
    PathHistoryList(Rect, usize),
}

/// Content shown by the full-screen text viewer when it is displaying a file
/// (as opposed to the last command's output).
struct ViewerContent {
    title: String,
    buf: TextBuffer,
    highlight: Option<(String, bool, bool, bool)>, // needle, case_sensitive, is_regex, whole_words
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
        let _ = execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen, DisableMouseCapture);
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
    last_output: Option<TextBuffer>,
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
    input_ok_btn: Cell<Button>,
    input_cancel_btn: Cell<Button>,
    input_cb_files_only: Cell<Option<Rect>>,
    input_cb_case_sensitive: Cell<Option<Rect>>,
    input_cb_regexp: Cell<Option<Rect>>,
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
    palette: Option<PaletteSession>,
    palette_popup_area: Cell<Rect>,
    palette_popup_offset: Cell<usize>,
    palette_commands: Vec<crate::palette::Command>,
    recent_actions: Vec<String>,
    quicksearch: Option<String>,
    shell_mode: ShellMode,
    needs_full_redraw: bool,
    ipc: Option<IpcServer>,
    status: StatusBarState,
    // Running async search (None when idle or finished); drained each tick.
    search: Option<Box<dyn SearchHandle>>,
    // True when the user toggled off the matches panel of a still-running/still-cached
    // content search (Action::ToggleMatchesPanel); distinguishes that from "no content
    // search at all", which also shows PanelContent::Dir on the companion side.
    matches_panel_hidden: bool,
    // Full-screen text viewer content (file mode); None = viewer closed.
    viewer: Option<ViewerContent>,
    // Search dialog hit-test areas (updated each render)
    search_cb_areas: Cell<[Option<Rect>; 7]>,
    search_input_areas: Cell<[Option<Rect>; 3]>,
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
            input_ok_btn: Cell::new(Button::default()),
            input_cancel_btn: Cell::new(Button::default()),
            input_cb_files_only: Cell::new(None),
            input_cb_case_sensitive: Cell::new(None),
            input_cb_regexp: Cell::new(None),
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
            palette: None,
            palette_popup_area: Cell::new(Rect::default()),
            palette_popup_offset: Cell::new(0),
            palette_commands: crate::palette::load_builtin_commands(),
            recent_actions: state.recent_actions.clone(),
            quicksearch: None,
            shell_mode: ShellMode::Stateless,
            needs_full_redraw: false,
            ipc: IpcServer::new(),
            status: StatusBarState::default(),
            search: None,
            matches_panel_hidden: false,
            viewer: None,
            search_cb_areas: Cell::new([None; 7]),
            search_input_areas: Cell::new([None; 3]),
            config,
        };
        // Try to spawn subshell if configured
        if app.config.startup.subshell {
            // Err falls back to stateless silently.
            if let Ok(sub) = crate::subshell::Subshell::spawn() {
                app.shell_mode = ShellMode::Subshell(sub);
                // spawn() drains the shell's freshly-emitted first prompt from the
                // PTY buffer, so the next passthrough entry must force a new one
                // (same reasoning as the post-stateless-command case below).
                app.subshell_prompt_needed = true;
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

    /// Directory to use as cwd for shell commands run against the active panel.
    /// A Matches panel isn't a directory browser: `panel.path` on it still holds
    /// whatever directory was there before the panel got replaced by the matches
    /// view, which is confusing as a cwd since that directory is now hidden. Use
    /// the directory of the file whose matches are shown instead.
    fn active_panel_shell_dir(&self) -> String {
        if let PanelContent::Matches(ms) = &self.active_panel().content {
            if let Some(dir) = ms.abs_path.as_deref().and_then(|p| Path::new(p).parent()) {
                return dir.to_string_lossy().into_owned();
            }
        }
        self.active_panel().path.0.clone()
    }

    fn panel(&self, side: Side) -> &PanelState {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn panel_mut(&mut self, side: Side) -> &mut PanelState {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    fn panel_history(&self, side: Side) -> &crate::panel_history::PanelHistory {
        match side {
            Side::Left => &self.panel_history_left,
            Side::Right => &self.panel_history_right,
        }
    }

    fn panel_history_mut(&mut self, side: Side) -> &mut crate::panel_history::PanelHistory {
        match side {
            Side::Left => &mut self.panel_history_left,
            Side::Right => &mut self.panel_history_right,
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

    fn bindings_list(&self) -> Vec<(&'static str, &ActionBindings, Action)> {
        let kb = &self.config.keybindings;
        vec![
            ("switch_panel", &kb.switch_panel, Action::SwitchPanel),
            ("toggle_layout", &kb.toggle_layout, Action::ToggleLayout),
            ("tag_file", &kb.tag_file, Action::TagFile),
            ("invert_tags", &kb.invert_tags, Action::InvertTags),
            ("copy", &kb.copy, Action::Copy),
            ("move", &kb.move_entry, Action::Move),
            ("delete", &kb.delete, Action::Delete),
            ("user_menu", &kb.user_menu, Action::UserMenu),
            ("exit", &kb.exit, Action::Exit),
            ("cmdline_insert_filename", &kb.cmdline_insert_filename, Action::CmdlineInsertFilename),
            ("cmdline_insert_fullpath", &kb.cmdline_insert_fullpath, Action::CmdlineInsertFullpath),
            ("cmdline_complete", &kb.cmdline_complete, Action::CmdlineComplete),
            ("cmdline_insert_tagged", &kb.cmdline_insert_tagged, Action::CmdlineInsertTagged),
            ("cmdline_insert_tagged_other", &kb.cmdline_insert_tagged_other, Action::CmdlineInsertTaggedOther),
            ("cmdline_insert_path", &kb.cmdline_insert_path, Action::CmdlineInsertPath),
            ("cmdline_insert_path_other", &kb.cmdline_insert_path_other, Action::CmdlineInsertPathOther),
            ("toggle_shell", &kb.toggle_shell, Action::ToggleShell),
            ("toggle_shell_and_sync_command_line", &kb.toggle_shell_and_sync_command_line, Action::ToggleShellAndSyncCommandLine),
            ("toggle_cmdline", &kb.toggle_cmdline, Action::ToggleCmdline),
            ("toggle_button_bar", &kb.toggle_button_bar, Action::ToggleButtonBar),
            ("cmdline_history_prev", &kb.cmdline_history_prev, Action::CmdlineHistoryPrev),
            ("cmdline_history_next", &kb.cmdline_history_next, Action::CmdlineHistoryNext),
            ("reverse_search", &kb.reverse_search, Action::ReverseSearch),
            ("sync_panels", &kb.sync_panels, Action::SyncPanels),
            ("rename", &kb.rename, Action::Rename),
            ("sort_panel", &kb.sort_panel, Action::SortPanel),
            ("quicksearch", &kb.quicksearch, Action::Quicksearch),
            ("toggle_hidden", &kb.toggle_hidden, Action::ToggleHidden),
            ("bookmark_open", &kb.bookmark_open, Action::BookmarkOpen),
            ("bookmark_add", &kb.bookmark_add, Action::BookmarkAdd),
            ("mkdir", &kb.mkdir, Action::Mkdir),
            ("path_history", &kb.path_history, Action::PathHistory),
            ("filter", &kb.filter, Action::Filter),
            ("search", &kb.search, Action::Search),
            ("select_group", &kb.select_group, Action::SelectGroup),
            ("unselect_group", &kb.unselect_group, Action::UnselectGroup),
            ("refresh_panel", &kb.refresh_panel, Action::RefreshPanel),
            ("go_to_parent", &kb.go_to_parent, Action::GoToParent),
            ("go_back", &kb.go_back, Action::GoBack),
            ("go_forward", &kb.go_forward, Action::GoForward),
            ("toggle_matches_panel", &kb.toggle_matches_panel, Action::ToggleMatchesPanel),
            ("view", &kb.view, Action::View),
            ("action_palette", &kb.action_palette, Action::ActionPalette),
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































































}









#[cfg(test)]
mod tests;
