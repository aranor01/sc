use crate::config::ColorScheme;
use crate::ui::cmdline::CmdLineState;
use crate::ui::focus::FocusRing;
use crate::ui::modal_event::ModalOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use super::button::Button;
use super::to_color;

// ── CheckboxOptions ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CheckboxOptions {
    pub files_only: bool,
    pub case_sensitive: bool,
    pub is_regexp: bool,
}

impl Default for CheckboxOptions {
    fn default() -> Self {
        CheckboxOptions { files_only: true, case_sensitive: true, is_regexp: false }
    }
}

// ── InputDialog ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDialogAction {
    Rename,
    Mkdir,
    Filter,
    SelectGroup,
    UnselectGroup,
}

// Focus ring indices for dialogs with checkboxes:
//   0 = text input, 1 = files_only, 2 = case_sensitive, 3 = regexp, 4 = ok, 5 = cancel
// Focus ring indices for dialogs without checkboxes:
//   0 = text input, 1 = ok, 2 = cancel
const FOCUS_CANCEL_WITH_CB: usize = 5;
const FOCUS_CANCEL_NO_CB: usize = 2;
const FOCUS_OK_WITH_CB: usize = 4;
const FOCUS_OK_NO_CB: usize = 1;

#[derive(Debug, Clone)]
pub struct InputDialogState {
    pub action: InputDialogAction,
    pub title: &'static str,
    pub input: CmdLineState,
    pub error: Option<String>,
    pub focus: FocusRing,
    pub checkboxes: Option<CheckboxOptions>,
}

impl InputDialogState {
    pub fn new(action: InputDialogAction, title: &'static str, prefill: &str) -> Self {
        let mut input = CmdLineState::new();
        input.text = prefill.to_string();
        input.cursor = prefill.len();
        InputDialogState {
            action,
            title,
            input,
            error: None,
            focus: FocusRing::new(3),
            checkboxes: None,
        }
    }

    pub fn new_pattern(
        action: InputDialogAction,
        title: &'static str,
        prefill: &str,
        opts: CheckboxOptions,
    ) -> Self {
        let mut input = CmdLineState::new();
        input.text = prefill.to_string();
        input.cursor = prefill.len();
        InputDialogState {
            action,
            title,
            input,
            error: None,
            focus: FocusRing::new(6),
            checkboxes: Some(opts),
        }
    }

    fn cancel_idx(&self) -> usize {
        if self.checkboxes.is_some() { FOCUS_CANCEL_WITH_CB } else { FOCUS_CANCEL_NO_CB }
    }

    fn ok_idx(&self) -> usize {
        if self.checkboxes.is_some() { FOCUS_OK_WITH_CB } else { FOCUS_OK_NO_CB }
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> ModalOutcome {
        // Tab / Shift-Tab: cycle focus
        if event.code == KeyCode::Tab && event.modifiers == KeyModifiers::NONE {
            self.focus.next();
            return ModalOutcome::Consumed;
        }
        if event.code == KeyCode::BackTab {
            self.focus.prev();
            return ModalOutcome::Consumed;
        }

        // Esc: always dismiss
        if event.code == KeyCode::Esc && event.modifiers == KeyModifiers::NONE {
            return ModalOutcome::Dismissed;
        }

        // Enter: confirm unless focus is on Cancel
        if event.code == KeyCode::Enter && event.modifiers == KeyModifiers::NONE {
            if self.focus.current() == self.cancel_idx() {
                return ModalOutcome::Dismissed;
            }
            return ModalOutcome::Confirmed;
        }

        // Space: toggle focused checkbox, or activate OK/Cancel
        if event.code == KeyCode::Char(' ') && event.modifiers == KeyModifiers::NONE {
            if let Some(ref mut cb) = self.checkboxes {
                match self.focus.current() {
                    1 => { cb.files_only = !cb.files_only; return ModalOutcome::Consumed; }
                    2 => { cb.case_sensitive = !cb.case_sensitive; return ModalOutcome::Consumed; }
                    3 => { cb.is_regexp = !cb.is_regexp; return ModalOutcome::Consumed; }
                    _ => {}
                }
            }
            let ok_idx = self.ok_idx();
            let cancel_idx = self.cancel_idx();
            if self.focus.current() == ok_idx { return ModalOutcome::Confirmed; }
            if self.focus.current() == cancel_idx { return ModalOutcome::Dismissed; }
        }

        // All other keys: delegate to input only when input is focused
        if self.focus.current() == 0 {
            self.input.handle_key(event);
            self.error = None;
        }
        ModalOutcome::Consumed
    }
}

pub struct InputDialogAreas {
    pub ok: Button,
    pub cancel: Button,
    pub cb_files_only: Option<Rect>,
    pub cb_case_sensitive: Option<Rect>,
    pub cb_regexp: Option<Rect>,
}

// ── Checkbox render helper ────────────────────────────────────────────────────

fn render_checkbox(
    x: u16,
    y: u16,
    buf: &mut Buffer,
    label: &str,
    checked: bool,
    focused: bool,
    cs: &ColorScheme,
) -> Rect {
    let bracket_fg = if focused { to_color(cs.dialog_border_fg) } else { to_color(cs.dialog_fg) };
    let bracket_style = Style::default().fg(bracket_fg).bg(to_color(cs.dialog_bg));
    let mark_style = Style::default()
        .fg(to_color(cs.dialog_mark_fg))
        .bg(to_color(cs.dialog_bg));
    let label_style = Style::default()
        .fg(if focused { to_color(cs.dialog_border_fg) } else { to_color(cs.dialog_fg) })
        .bg(to_color(cs.dialog_bg));

    buf.set_string(x, y, "[", bracket_style);
    if checked {
        buf.set_string(x + 1, y, "x", mark_style);
    } else {
        buf.set_string(x + 1, y, " ", bracket_style);
    }
    buf.set_string(x + 2, y, "] ", bracket_style);
    buf.set_string(x + 4, y, label, label_style);

    Rect { x, y, width: (4 + label.len()) as u16, height: 1 }
}

// ── Text input render helper ──────────────────────────────────────────────────

fn render_text_input(
    area: Rect,
    buf: &mut Buffer,
    state: &CmdLineState,
    fg: Color,
    bg: Color,
) -> Option<Position> {
    let style = Style::default().fg(fg).bg(bg);
    let blank: String = " ".repeat(area.width as usize);
    buf.set_string(area.x, area.y, &blank, style);

    let cursor_char = state.text[..state.cursor].chars().count();
    let w = area.width as usize;
    let scroll = if cursor_char >= w { cursor_char + 1 - w } else { 0 };

    let visible: String = state.text.chars().skip(scroll).take(w).collect();
    buf.set_string(area.x, area.y, &visible, style);

    let cursor_col = (cursor_char - scroll) as u16;
    if cursor_col < area.width {
        Some(Position { x: area.x + cursor_col, y: area.y })
    } else {
        None
    }
}

pub fn render_input_dialog(
    area: Rect,
    buf: &mut Buffer,
    cs: &ColorScheme,
    state: &InputDialogState,
    press: Option<Position>,
) -> (InputDialogAreas, Option<Position>) {
    let has_cb = state.checkboxes.is_some();
    // height: 2 border + 1 input + 1 error + (3 cb rows + 1 gap if has_cb) + 1 buttons
    let height = if has_cb { 9u16 } else { 5u16 };
    let width = 52u16.min(area.width.saturating_sub(2));
    let dialog_area = centered_rect(width, height, area);

    Widget::render(Clear, dialog_area, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(to_color(cs.dialog_border_fg)))
        .title(Span::styled(
            state.title,
            Style::default().fg(to_color(cs.dialog_fg)).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(to_color(cs.dialog_bg)));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    // Input field — use dialog_border_fg bg when focused to highlight
    let input_focused = state.focus.is_focused(0);
    let (input_fg, input_bg) = if input_focused {
        (to_color(cs.cmdline_fg), to_color(cs.cmdline_bg))
    } else {
        (to_color(cs.cmdline_inactive_fg), to_color(cs.cmdline_inactive_bg))
    };
    let input_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 };
    let cursor_pos = render_text_input(input_area, buf, &state.input, input_fg, input_bg);
    // Only expose cursor when input field is focused
    let cursor_out = if input_focused { cursor_pos } else { None };

    // Error line (inner.y + 1)
    if let Some(ref err) = state.error {
        let err_style = Style::default().fg(to_color(cs.dialog_error_fg)).bg(to_color(cs.dialog_bg));
        let truncated: String = err.chars().take(inner.width as usize).collect();
        buf.set_string(inner.x, inner.y + 1, &truncated, err_style);
    }

    // Checkboxes (inner.y + 2..4) and buttons row
    let (cb_fo_rect, cb_cs_rect, cb_re_rect, button_row) = if let Some(ref cb) = state.checkboxes {
        let cb_x = inner.x + 1;
        let fo = render_checkbox(
            cb_x, inner.y + 2, buf, "Files only",
            cb.files_only, state.focus.is_focused(1), cs,
        );
        let cs_r = render_checkbox(
            cb_x, inner.y + 3, buf, "Case sensitive",
            cb.case_sensitive, state.focus.is_focused(2), cs,
        );
        let re = render_checkbox(
            cb_x, inner.y + 4, buf, "RegExp",
            cb.is_regexp, state.focus.is_focused(3), cs,
        );
        (Some(fo), Some(cs_r), Some(re), inner.y + 6)
    } else {
        (None, None, None, inner.y + 2)
    };

    // Buttons
    const OK_LABEL: &str = "[ OK ]";
    const CANCEL_LABEL: &str = "[Cancel]";

    let (ok_focus_idx, cancel_focus_idx) = if has_cb { (4, 5) } else { (1, 2) };

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        const BUTTONS_TOTAL: u16 = (OK_LABEL.len() + 2 + CANCEL_LABEL.len()) as u16;
        let ok_x = inner.x + inner.width.saturating_sub(BUTTONS_TOTAL) / 2;
        let ok_btn = Button::build(OK_LABEL, ok_x, button_row, cs);
        let cancel_btn = Button::build(CANCEL_LABEL, ok_x + OK_LABEL.len() as u16 + 2, button_row, cs);
        ok_btn.render_state(OK_LABEL, buf,
            state.focus.is_focused(ok_focus_idx) || ok_btn.is_pressed(press));
        cancel_btn.render_state(CANCEL_LABEL, buf,
            state.focus.is_focused(cancel_focus_idx) || cancel_btn.is_pressed(press));
        (InputDialogAreas {
            ok: ok_btn,
            cancel: cancel_btn,
            cb_files_only: cb_fo_rect,
            cb_case_sensitive: cb_cs_rect,
            cb_regexp: cb_re_rect,
        }, cursor_out)
    } else {
        (InputDialogAreas {
            ok: Button::default(),
            cancel: Button::default(),
            cb_files_only: None,
            cb_case_sensitive: None,
            cb_regexp: None,
        }, cursor_out)
    }
}

// ── SearchDialog ──────────────────────────────────────────────────────────────

// Focus ring indices:
//   0 = pattern input, 1 = content input, 2 = depth input,
//   3 = RegExp, 4 = Case sensitive, 5 = Include hidden, 6 = Follow symlinks,
//   7 = OK, 8 = Cancel
const SEARCH_FOCUS_OK: usize = 7;
const SEARCH_FOCUS_CANCEL: usize = 8;
const SEARCH_FOCUS_COUNT: usize = 9;

#[derive(Debug, Clone)]
pub struct SearchDialogState {
    pub pattern: CmdLineState,
    pub content: CmdLineState,
    pub depth: CmdLineState,
    pub is_regexp: bool,
    pub case_sensitive: bool,
    pub include_hidden: bool,
    pub follow_symlinks: bool,
    pub error: Option<String>,
    pub focus: FocusRing,
}

impl SearchDialogState {
    pub fn new(include_hidden: bool) -> Self {
        SearchDialogState {
            pattern: CmdLineState::new(),
            content: CmdLineState::new(),
            depth: CmdLineState::new(),
            is_regexp: false,
            case_sensitive: true,
            include_hidden,
            follow_symlinks: false,
            error: None,
            focus: FocusRing::new(SEARCH_FOCUS_COUNT),
        }
    }

    fn focused_input(&mut self) -> Option<&mut CmdLineState> {
        match self.focus.current() {
            0 => Some(&mut self.pattern),
            1 => Some(&mut self.content),
            2 => Some(&mut self.depth),
            _ => None,
        }
    }

    pub fn toggle_checkbox(&mut self, idx: usize) {
        match idx {
            3 => self.is_regexp = !self.is_regexp,
            4 => self.case_sensitive = !self.case_sensitive,
            5 => self.include_hidden = !self.include_hidden,
            6 => self.follow_symlinks = !self.follow_symlinks,
            _ => {}
        }
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> ModalOutcome {
        if event.code == KeyCode::Tab && event.modifiers == KeyModifiers::NONE {
            self.focus.next();
            return ModalOutcome::Consumed;
        }
        if event.code == KeyCode::BackTab {
            self.focus.prev();
            return ModalOutcome::Consumed;
        }
        if event.code == KeyCode::Esc && event.modifiers == KeyModifiers::NONE {
            return ModalOutcome::Dismissed;
        }
        if event.code == KeyCode::Enter && event.modifiers == KeyModifiers::NONE {
            if self.focus.current() == SEARCH_FOCUS_CANCEL {
                return ModalOutcome::Dismissed;
            }
            return ModalOutcome::Confirmed;
        }
        if event.code == KeyCode::Char(' ') && event.modifiers == KeyModifiers::NONE {
            match self.focus.current() {
                idx @ 3..=6 => {
                    self.toggle_checkbox(idx);
                    return ModalOutcome::Consumed;
                }
                SEARCH_FOCUS_OK => return ModalOutcome::Confirmed,
                SEARCH_FOCUS_CANCEL => return ModalOutcome::Dismissed,
                _ => {} // space falls through into the focused text input
            }
        }
        // The depth field accepts digits only.
        if self.focus.current() == 2 {
            if let KeyCode::Char(c) = event.code {
                if !c.is_ascii_digit() {
                    return ModalOutcome::Consumed;
                }
            }
        }
        if let Some(input) = self.focused_input() {
            input.handle_key(event);
            self.error = None;
        }
        ModalOutcome::Consumed
    }
}

pub struct SearchDialogAreas {
    pub ok: Button,
    pub cancel: Button,
    /// RegExp, Case sensitive, Include hidden, Follow symlinks.
    pub checkboxes: [Option<Rect>; 4],
    /// Pattern, content, depth input rows (click to focus).
    pub inputs: [Option<Rect>; 3],
}

pub fn render_search_dialog(
    area: Rect,
    buf: &mut Buffer,
    cs: &ColorScheme,
    state: &SearchDialogState,
    press: Option<Position>,
) -> (SearchDialogAreas, Option<Position>) {
    // height: 2 border + 3 inputs + 1 error + 2 checkbox rows + 1 gap + 1 buttons
    let height = 10u16;
    let width = 52u16.min(area.width.saturating_sub(2));
    let dialog_area = centered_rect(width, height, area);

    Widget::render(Clear, dialog_area, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(to_color(cs.dialog_border_fg)))
        .title(Span::styled(
            " Search ",
            Style::default().fg(to_color(cs.dialog_fg)).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(to_color(cs.dialog_bg)));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    let label_style = Style::default().fg(to_color(cs.dialog_fg)).bg(to_color(cs.dialog_bg));
    const LABEL_W: u16 = 17;

    let mut cursor_out: Option<Position> = None;
    let mut inputs: [Option<Rect>; 3] = [None; 3];
    let rows: [(&str, &CmdLineState, usize); 3] = [
        ("File pattern:", &state.pattern, 0),
        ("Containing text:", &state.content, 1),
        ("Max depth:", &state.depth, 2),
    ];
    for (i, (label, input, focus_idx)) in rows.into_iter().enumerate() {
        let y = inner.y + i as u16;
        buf.set_string(inner.x + 1, y, label, label_style);
        let focused = state.focus.is_focused(focus_idx);
        let (fg, bg) = if focused {
            (to_color(cs.cmdline_fg), to_color(cs.cmdline_bg))
        } else {
            (to_color(cs.cmdline_inactive_fg), to_color(cs.cmdline_inactive_bg))
        };
        let input_area = Rect {
            x: inner.x + 1 + LABEL_W,
            y,
            width: inner.width.saturating_sub(2 + LABEL_W),
            height: 1,
        };
        let pos = render_text_input(input_area, buf, input, fg, bg);
        if focused {
            cursor_out = pos;
        }
        inputs[i] = Some(input_area);
    }

    // Error line
    if let Some(ref err) = state.error {
        let err_style = Style::default().fg(to_color(cs.dialog_error_fg)).bg(to_color(cs.dialog_bg));
        let truncated: String = err.chars().take(inner.width as usize).collect();
        buf.set_string(inner.x, inner.y + 3, &truncated, err_style);
    }

    // Checkboxes, two per row
    let cb_x = inner.x + 1;
    let cb_x2 = inner.x + inner.width / 2;
    let cb_re = render_checkbox(cb_x, inner.y + 4, buf, "RegExp",
        state.is_regexp, state.focus.is_focused(3), cs);
    let cb_cs = render_checkbox(cb_x2, inner.y + 4, buf, "Case sensitive",
        state.case_sensitive, state.focus.is_focused(4), cs);
    let cb_hid = render_checkbox(cb_x, inner.y + 5, buf, "Include hidden",
        state.include_hidden, state.focus.is_focused(5), cs);
    let cb_sym = render_checkbox(cb_x2, inner.y + 5, buf, "Follow symlinks",
        state.follow_symlinks, state.focus.is_focused(6), cs);

    // Buttons
    const OK_LABEL: &str = "[ OK ]";
    const CANCEL_LABEL: &str = "[Cancel]";
    let button_row = inner.y + 7;
    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        const BUTTONS_TOTAL: u16 = (OK_LABEL.len() + 2 + CANCEL_LABEL.len()) as u16;
        let ok_x = inner.x + inner.width.saturating_sub(BUTTONS_TOTAL) / 2;
        let ok_btn = Button::build(OK_LABEL, ok_x, button_row, cs);
        let cancel_btn = Button::build(CANCEL_LABEL, ok_x + OK_LABEL.len() as u16 + 2, button_row, cs);
        ok_btn.render_state(OK_LABEL, buf,
            state.focus.is_focused(SEARCH_FOCUS_OK) || ok_btn.is_pressed(press));
        cancel_btn.render_state(CANCEL_LABEL, buf,
            state.focus.is_focused(SEARCH_FOCUS_CANCEL) || cancel_btn.is_pressed(press));
        (SearchDialogAreas {
            ok: ok_btn,
            cancel: cancel_btn,
            checkboxes: [Some(cb_re), Some(cb_cs), Some(cb_hid), Some(cb_sym)],
            inputs,
        }, cursor_out)
    } else {
        (SearchDialogAreas {
            ok: Button::default(),
            cancel: Button::default(),
            checkboxes: [None; 4],
            inputs: [None; 3],
        }, cursor_out)
    }
}

// ── ConfirmState ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ConfirmOp {
    Copy,
    Move,
    Delete,
}

#[derive(Debug, Clone)]
pub struct ConfirmState {
    pub op: ConfirmOp,
    pub files: Vec<String>,
    pub dst: Option<String>,
    pub focus: usize, // 0 = Yes, 1 = No
}

impl ConfirmState {
    pub fn new(op: ConfirmOp, files: Vec<String>, dst: Option<String>) -> Self {
        ConfirmState { op, files, dst, focus: 0 }
    }

    pub fn title(&self) -> &'static str {
        match self.op {
            ConfirmOp::Copy => " Copy ",
            ConfirmOp::Move => " Move ",
            ConfirmOp::Delete => " Delete ",
        }
    }

    pub fn message(&self) -> String {
        match &self.op {
            ConfirmOp::Delete => {
                if self.files.len() == 1 {
                    format!("Delete '{}'?", self.files[0])
                } else {
                    format!("Delete {} files?", self.files.len())
                }
            }
            ConfirmOp::Copy => {
                let dst = self.dst.as_deref().unwrap_or("?");
                if self.files.len() == 1 {
                    format!("Copy '{}'\nto '{}'?", self.files[0], dst)
                } else {
                    format!("Copy {} files\nto '{}'?", self.files.len(), dst)
                }
            }
            ConfirmOp::Move => {
                let dst = self.dst.as_deref().unwrap_or("?");
                if self.files.len() == 1 {
                    format!("Move '{}'\nto '{}'?", self.files[0], dst)
                } else {
                    format!("Move {} files\nto '{}'?", self.files.len(), dst)
                }
            }
        }
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> ModalOutcome {
        // Tab / Shift-Tab cycle Yes/No
        if event.code == KeyCode::Tab && event.modifiers == KeyModifiers::NONE
            || event.code == KeyCode::BackTab
        {
            self.focus = 1 - self.focus;
            return ModalOutcome::Consumed;
        }
        // Enter activates the focused button
        if event.code == KeyCode::Enter && event.modifiers == KeyModifiers::NONE {
            return if self.focus == 0 { ModalOutcome::Confirmed } else { ModalOutcome::Dismissed };
        }
        // Shortcut keys (kept for muscle memory)
        if matches!(event.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
            return ModalOutcome::Confirmed;
        }
        if matches!(event.code, KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc) {
            return ModalOutcome::Dismissed;
        }
        ModalOutcome::Consumed
    }
}

pub struct ConfirmButtonAreas {
    pub yes: Button,
    pub no: Button,
}

pub struct ErrorButtonArea {
    pub ok: Button,
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

pub fn render_confirm(
    area: Rect,
    buf: &mut Buffer,
    cs: &ColorScheme,
    state: &ConfirmState,
    press: Option<Position>,
) -> ConfirmButtonAreas {
    let msg = state.message();
    let line_count = msg.lines().count() as u16;
    // border(2) + message lines + blank line + button line
    let height = line_count + 4;
    let width = 44u16.min(area.width.saturating_sub(2));
    let dialog_area = centered_rect(width, height, area);

    Widget::render(Clear, dialog_area, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(to_color(cs.dialog_border_fg)))
        .title(Span::styled(
            state.title(),
            Style::default()
                .fg(to_color(cs.dialog_fg))
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(to_color(cs.dialog_bg)));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    // Render message text
    let msg_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: line_count.min(inner.height),
    };
    let para = Paragraph::new(msg.as_str())
        .style(Style::default().fg(to_color(cs.dialog_fg)).bg(to_color(cs.dialog_bg)))
        .wrap(Wrap { trim: false });
    Widget::render(para, msg_area, buf);

    // Buttons — highlight the focused one
    let button_row = inner.y + line_count + 1;
    const YES_LABEL: &str = "[ Yes ]";
    const YES_ACCESS_KEY: char = 'Y';
    const NO_LABEL: &str = "[ No ]";
    const NO_ACCESS_KEY: char = 'N';
    const BUTTONS_TOTAL: u16 = (YES_LABEL.len() + 2 + NO_LABEL.len()) as u16;
    let yes_x = inner.x + inner.width.saturating_sub(BUTTONS_TOTAL) / 2;
    let yes_btn = Button::build_with_access_key(YES_LABEL, yes_x, button_row, cs, YES_ACCESS_KEY);
    let no_btn = Button::build_with_access_key(NO_LABEL, yes_x + YES_LABEL.len() as u16 + 2, button_row, cs, NO_ACCESS_KEY);

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        yes_btn.render_state(YES_LABEL, buf, state.focus == 0 || yes_btn.is_pressed(press));
        no_btn.render_state(NO_LABEL, buf, state.focus == 1 || no_btn.is_pressed(press));
        ConfirmButtonAreas { yes: yes_btn, no: no_btn }
    } else {
        ConfirmButtonAreas { yes: Button::default(), no: Button::default() }
    }
}

// ratatui's Paragraph::line_count() would do this via the real wrapper, but it's gated
// behind the unstable `unstable-rendered-line-info` feature (ratatui#293), so approximate
// wrapping with a plain width division instead.
fn visual_line_count(text: &str, width: u16) -> u16 {
    if width == 0 {
        return text.lines().count() as u16;
    }
    text.lines()
        .map(|line| {
            let w = line.width() as u16;
            w.div_ceil(width).max(1)
        })
        .sum::<u16>()
        .max(1)
}

pub fn render_error(
    area: Rect,
    buf: &mut Buffer,
    cs: &ColorScheme,
    message: &str,
    _press: Option<Position>,
) -> ErrorButtonArea {
    let width = 50u16.min(area.width.saturating_sub(2));
    let inner_width = width.saturating_sub(2);
    let line_count = visual_line_count(message, inner_width);
    // border(2) + message lines + blank line + button line
    let height = line_count + 4;
    let dialog_area = centered_rect(width, height, area);

    Widget::render(Clear, dialog_area, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(to_color(cs.dialog_error_fg)))
        .title(Span::styled(
            " Error ",
            Style::default().fg(to_color(cs.dialog_error_fg)).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(to_color(cs.dialog_bg)));

    let inner = block.inner(dialog_area);
    block.render(dialog_area, buf);

    // Render message text
    let msg_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: line_count.min(inner.height),
    };
    let para = Paragraph::new(message)
        .style(Style::default().fg(to_color(cs.dialog_fg)).bg(to_color(cs.dialog_bg)))
        .wrap(Wrap { trim: false });
    Widget::render(para, msg_area, buf);

    // Centered [ OK ] button — always shown as focused (only button)
    const OK_LABEL: &str = "[ OK ]";
    let button_row = inner.y + line_count + 1;
    let ok_x = inner.x + inner.width.saturating_sub(OK_LABEL.len() as u16) / 2;
    let ok_btn = Button::build(OK_LABEL, ok_x, button_row, cs);

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        ok_btn.render_state(OK_LABEL, buf, true);
        ErrorButtonArea { ok: ok_btn }
    } else {
        ErrorButtonArea { ok: Button::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn search_dialog_focus_ring_cycles_through_all_stops() {
        let mut s = SearchDialogState::new(false);
        assert_eq!(s.focus.current(), 0);
        for _ in 0..SEARCH_FOCUS_COUNT {
            s.handle_key(&key(KeyCode::Tab));
        }
        assert_eq!(s.focus.current(), 0);
    }

    #[test]
    fn search_dialog_space_toggles_checkboxes_but_types_in_inputs() {
        let mut s = SearchDialogState::new(false);
        s.handle_key(&key(KeyCode::Char(' ')));
        assert_eq!(s.pattern.text, " ");
        s.focus.set(3);
        s.handle_key(&key(KeyCode::Char(' ')));
        assert!(s.is_regexp);
        s.focus.set(5);
        s.handle_key(&key(KeyCode::Char(' ')));
        assert!(s.include_hidden);
    }

    #[test]
    fn search_dialog_depth_accepts_digits_only() {
        let mut s = SearchDialogState::new(false);
        s.focus.set(2);
        s.handle_key(&key(KeyCode::Char('a')));
        s.handle_key(&key(KeyCode::Char('3')));
        s.handle_key(&key(KeyCode::Char('-')));
        assert_eq!(s.depth.text, "3");
    }

    #[test]
    fn search_dialog_enter_confirms_except_on_cancel() {
        let mut s = SearchDialogState::new(false);
        assert!(matches!(s.handle_key(&key(KeyCode::Enter)), ModalOutcome::Confirmed));
        s.focus.set(SEARCH_FOCUS_CANCEL);
        assert!(matches!(s.handle_key(&key(KeyCode::Enter)), ModalOutcome::Dismissed));
        assert!(matches!(s.handle_key(&key(KeyCode::Esc)), ModalOutcome::Dismissed));
    }

    #[test]
    fn search_dialog_include_hidden_preseeded() {
        assert!(SearchDialogState::new(true).include_hidden);
        assert!(!SearchDialogState::new(false).include_hidden);
    }
}
