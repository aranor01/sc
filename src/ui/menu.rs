use crate::config::{ColorScheme, MenuItem};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, StatefulWidget, Widget},
};

use super::to_color;

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

pub struct UserMenuAreas {
    pub list_area: Rect,
    pub list_offset: usize,
    pub close: Rect,
}

pub struct UserMenuWidget<'a> {
    pub cs: &'a ColorScheme,
}

impl<'a> UserMenuWidget<'a> {
    pub fn render_in(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &mut UserMenuState,
    ) -> UserMenuAreas {
        let width = 40u16.min(area.width.saturating_sub(4));
        // border(2) + items + close button(1)
        let height = (state.items.len() as u16 + 3).min(area.height.saturating_sub(2));
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

        // Reserve the last inner row for the Close button
        let list_height = inner.height.saturating_sub(1);
        let list_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: list_height,
        };

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

        StatefulWidget::render(list, list_area, buf, &mut list_state);

        let list_offset = list_state.offset();

        // Render centered Close button on the last inner row
        const CLOSE_LABEL: &str = "[ Close ]";
        let close_row = inner.y + list_height;
        let close_x = inner.x + inner.width.saturating_sub(CLOSE_LABEL.len() as u16) / 2;
        let close_style = Style::default()
            .fg(to_color(self.cs.dialog_fg))
            .bg(to_color(self.cs.dialog_bg))
            .add_modifier(Modifier::BOLD);

        if close_row < dialog_area.y + dialog_area.height.saturating_sub(1) {
            buf.set_string(close_x, close_row, CLOSE_LABEL, close_style);
        }

        UserMenuAreas {
            list_area,
            list_offset,
            close: Rect {
                x: close_x,
                y: close_row,
                width: CLOSE_LABEL.len() as u16,
                height: 1,
            },
        }
    }
}
