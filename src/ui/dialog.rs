use crate::config::ColorScheme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use super::to_color;

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
}

pub struct ConfirmButtonAreas {
    pub yes: Rect,
    pub no: Rect,
}

pub struct ErrorButtonArea {
    pub ok: Rect,
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

    // Render message text (without buttons)
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

    // Render buttons on the row after the blank line
    let button_row = inner.y + line_count + 1;
    let btn_style = Style::default()
        .fg(to_color(cs.dialog_fg))
        .bg(to_color(cs.dialog_bg))
        .add_modifier(Modifier::BOLD);

    const YES_LABEL: &str = "[Y]es";
    const NO_LABEL: &str = "[N]o";
    let yes_x = inner.x + 1;
    let no_x = yes_x + YES_LABEL.len() as u16 + 2;

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        buf.set_string(yes_x, button_row, YES_LABEL, btn_style);
        buf.set_string(no_x, button_row, NO_LABEL, btn_style);
    }

    ConfirmButtonAreas {
        yes: Rect { x: yes_x, y: button_row, width: YES_LABEL.len() as u16, height: 1 },
        no: Rect { x: no_x, y: button_row, width: NO_LABEL.len() as u16, height: 1 },
    }
}

pub fn render_error(
    area: Rect,
    buf: &mut Buffer,
    cs: &ColorScheme,
    message: &str,
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

    // Render message text (without button)
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

    // Render centered [ OK ] button
    const OK_LABEL: &str = "[ OK ]";
    let button_row = inner.y + line_count + 1;
    let ok_x = inner.x + inner.width.saturating_sub(OK_LABEL.len() as u16) / 2;
    let btn_style = Style::default()
        .fg(to_color(cs.dialog_fg))
        .bg(to_color(cs.dialog_bg))
        .add_modifier(Modifier::BOLD);

    if button_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
        buf.set_string(ok_x, button_row, OK_LABEL, btn_style);
    }

    ErrorButtonArea {
        ok: Rect { x: ok_x, y: button_row, width: OK_LABEL.len() as u16, height: 1 },
    }
}
