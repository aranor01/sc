use crate::config::ColorScheme;
use crate::ui::cmdline::CmdLineState;
use crate::ui::modal_event::ModalOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use super::button::Button;
use super::to_color;

// ── InputDialog ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDialogAction {
    Rename,
    Mkdir,
    Filter,
    SelectGroup,
    UnselectGroup,
}

#[derive(Debug, Clone)]
pub struct InputDialogState {
    pub action: InputDialogAction,
    pub title: &'static str,
    pub input: CmdLineState,
    pub error: Option<String>,
}

impl InputDialogState {
    pub fn new(action: InputDialogAction, title: &'static str, prefill: &str) -> Self {
        let mut input = CmdLineState::new();
        input.text = prefill.to_string();
        input.cursor = prefill.len();
        InputDialogState { action, title, input, error: None }
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> ModalOutcome {
        if event.code == KeyCode::Enter && event.modifiers == KeyModifiers::NONE {
            return ModalOutcome::Confirmed;
        }
        if event.code == KeyCode::Esc && event.modifiers == KeyModifiers::NONE {
            return ModalOutcome::Dismissed;
        }
        self.input.handle_key(event);
        self.error = None;
        ModalOutcome::Consumed
    }
}

pub struct InputDialogAreas {
    pub ok: Button,
    pub cancel: Button,
}

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
    // height: 2 border + 1 input + 1 error/blank + 1 gap + 1 buttons = 6
    let height = 6u16;
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

    // Input field with cmdline background for contrast
    let input_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 };
    let cursor_pos = render_text_input(
        input_area,
        buf,
        &state.input,
        to_color(cs.cmdline_fg),
        to_color(cs.cmdline_bg),
    );

    // Error line (row 1 inside inner)
    if let Some(ref err) = state.error {
        let err_style = Style::default().fg(Color::Red).bg(to_color(cs.dialog_bg));
        let truncated: String = err.chars().take(inner.width as usize).collect();
        buf.set_string(inner.x, inner.y + 1, &truncated, err_style);
    }

    // Buttons (row 3 inside inner, row 2 is gap)
    let button_row = inner.y + 3;
    const OK_LABEL: &str = "[ OK ]";
    const CANCEL_LABEL: &str = "[Cancel]";

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        let ok_btn = Button::build(OK_LABEL, inner.x + 1, button_row, cs);
        let cancel_btn = Button::build(
            CANCEL_LABEL,
            ok_btn.area.x + OK_LABEL.len() as u16 + 2,
            button_row,
            cs,
        );
        ok_btn.render(OK_LABEL, buf, press);
        cancel_btn.render(CANCEL_LABEL, buf, press);
        (InputDialogAreas { ok: ok_btn, cancel: cancel_btn }, cursor_pos)
    } else {
        (InputDialogAreas { ok: Button::default(), cancel: Button::default() }, cursor_pos)
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
}

impl ConfirmState {
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

    pub fn handle_key(&self, event: &KeyEvent) -> ModalOutcome {
        if matches!(event.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
            ModalOutcome::Confirmed
        } else if matches!(event.code, KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc) {
            ModalOutcome::Dismissed
        } else {
            ModalOutcome::Consumed
        }
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

    // Buttons
    let button_row = inner.y + line_count + 1;
    const YES_LABEL: &str = "[Y]es";
    const NO_LABEL: &str = "[N]o";
    let yes_btn = Button::build(YES_LABEL, inner.x + 1, button_row, cs);
    let no_btn = Button::build(NO_LABEL, yes_btn.area.x + YES_LABEL.len() as u16 + 2, button_row, cs);

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        yes_btn.render(YES_LABEL, buf, press);
        no_btn.render(NO_LABEL, buf, press);
        ConfirmButtonAreas { yes: yes_btn, no: no_btn }
    } else {
        ConfirmButtonAreas { yes: Button::default(), no: Button::default() }
    }
}

pub fn render_error(
    area: Rect,
    buf: &mut Buffer,
    cs: &ColorScheme,
    message: &str,
    press: Option<Position>,
) -> ErrorButtonArea {
    let line_count = message.lines().count() as u16;
    // border(2) + message lines + blank line + button line
    let height = line_count + 4;
    let width = 50u16.min(area.width.saturating_sub(2));
    let dialog_area = centered_rect(width, height, area);

    Widget::render(Clear, dialog_area, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(Span::styled(
            " Error ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
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

    // Centered [ OK ] button
    const OK_LABEL: &str = "[ OK ]";
    let button_row = inner.y + line_count + 1;
    let ok_x = inner.x + inner.width.saturating_sub(OK_LABEL.len() as u16) / 2;
    let ok_btn = Button::build(OK_LABEL, ok_x, button_row, cs);

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        ok_btn.render(OK_LABEL, buf, press);
        ErrorButtonArea { ok: ok_btn }
    } else {
        ErrorButtonArea { ok: Button::default() }
    }
}
