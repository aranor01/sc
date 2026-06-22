use crate::config::ColorScheme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

fn to_color(c: crate::config::Color) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

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

pub fn render_confirm(area: Rect, buf: &mut Buffer, cs: &ColorScheme, state: &ConfirmState) {
    let msg = state.message();
    let line_count = msg.lines().count() as u16;
    let height = line_count + 4; // border + message lines + empty + hint
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

    let text = format!("{}\n\n[Y]es  [N]o", msg);
    let para = Paragraph::new(text)
        .style(Style::default().fg(to_color(cs.dialog_fg)).bg(to_color(cs.dialog_bg)))
        .wrap(Wrap { trim: false });
    Widget::render(para, inner, buf);
}

pub fn render_error(area: Rect, buf: &mut Buffer, cs: &ColorScheme, message: &str) {
    let line_count = message.lines().count() as u16;
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

    let text = format!("{}\n\n[Enter/Esc] OK", message);
    let para = Paragraph::new(text)
        .style(Style::default().fg(to_color(cs.dialog_fg)).bg(to_color(cs.dialog_bg)))
        .wrap(Wrap { trim: false });
    Widget::render(para, inner, buf);
}
