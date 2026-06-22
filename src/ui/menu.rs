use crate::config::{ColorScheme, MenuItem};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, StatefulWidget, Widget},
};

fn to_color(c: crate::config::Color) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

pub struct UserMenuState {
    pub items: Vec<MenuItem>,
    pub cursor: usize,
}

impl UserMenuState {
    pub fn new(items: Vec<MenuItem>) -> Self {
        UserMenuState { items, cursor: 0 }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.items.is_empty() && self.cursor < self.items.len() - 1 {
            self.cursor += 1;
        }
    }

    pub fn selected(&self) -> Option<&MenuItem> {
        self.items.get(self.cursor)
    }
}

pub struct UserMenuWidget<'a> {
    pub cs: &'a ColorScheme,
}

impl<'a> UserMenuWidget<'a> {
    pub fn render_in(&self, area: Rect, buf: &mut Buffer, state: &mut UserMenuState) {
        let width = 40u16.min(area.width.saturating_sub(4));
        let height = (state.items.len() as u16 + 2).min(area.height.saturating_sub(2));
        let x = (area.x + area.width / 2).saturating_sub(width / 2);
        let y = area.y + 2;
        let dialog_area = Rect { x, y, width, height };

        Widget::render(Clear, dialog_area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(to_color(self.cs.dialog_border_fg)))
            .title(Span::styled(
                " User Menu ",
                Style::default().fg(to_color(self.cs.dialog_fg)),
            ))
            .style(Style::default().bg(to_color(self.cs.dialog_bg)));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        let items: Vec<ListItem> = state
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let label = if let Some(keys) = &item.keys {
                    format!("{:<8} {}", keys, item.label)
                } else {
                    format!("         {}", item.label)
                };
                let style = if i == state.cursor {
                    Style::default()
                        .fg(to_color(self.cs.selected_fg))
                        .bg(to_color(self.cs.selected_bg))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(to_color(self.cs.dialog_fg))
                        .bg(to_color(self.cs.dialog_bg))
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(state.cursor));

        let list = List::new(items)
            .style(Style::default().bg(to_color(self.cs.dialog_bg)))
            .highlight_style(
                Style::default()
                    .fg(to_color(self.cs.selected_fg))
                    .bg(to_color(self.cs.selected_bg))
                    .add_modifier(Modifier::BOLD),
            );

        StatefulWidget::render(list, inner, buf, &mut list_state);
    }
}
