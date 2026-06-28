use crate::config::ColorScheme;
use crate::ui::modal_event::PopupOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{block::Title, Block, Borders, Clear, List, ListItem, ListState, StatefulWidget, Widget},
};

use super::to_color;

/// Generic selectable list state used by both completion and reverse-i-search popups.
pub struct PopupListState {
    pub items: Vec<String>,
    pub selected: usize,
}

impl PopupListState {
    pub fn new(items: Vec<String>) -> Self {
        PopupListState { items, selected: 0 }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if !self.items.is_empty() && self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    pub fn move_top(&mut self) {
        self.selected = 0;
    }

    pub fn move_bottom(&mut self) {
        if !self.items.is_empty() {
            self.selected = self.items.len() - 1;
        }
    }

    pub fn page_up(&mut self, page: usize) {
        self.selected = self.selected.saturating_sub(page);
    }

    pub fn page_down(&mut self, page: usize) {
        if !self.items.is_empty() {
            self.selected = (self.selected + page).min(self.items.len() - 1);
        }
    }

    pub fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected).map(String::as_str)
    }

    pub fn handle_key(&mut self, event: &KeyEvent, visible_height: usize) -> PopupOutcome {
        match event.code {
            KeyCode::Enter | KeyCode::Tab if event.modifiers == KeyModifiers::NONE => {
                match self.selected_item() {
                    Some(s) => PopupOutcome::Accept(s.to_string()),
                    None => PopupOutcome::Dismissed,
                }
            }
            KeyCode::Esc if event.modifiers == KeyModifiers::NONE => PopupOutcome::Dismissed,
            KeyCode::Up if event.modifiers == KeyModifiers::NONE => {
                self.move_up(); PopupOutcome::Consumed
            }
            KeyCode::Down if event.modifiers == KeyModifiers::NONE => {
                self.move_down(); PopupOutcome::Consumed
            }
            KeyCode::Home if event.modifiers == KeyModifiers::NONE => {
                self.move_top(); PopupOutcome::Consumed
            }
            KeyCode::End if event.modifiers == KeyModifiers::NONE => {
                self.move_bottom(); PopupOutcome::Consumed
            }
            KeyCode::PageUp if event.modifiers == KeyModifiers::NONE => {
                self.page_up(visible_height.max(1)); PopupOutcome::Consumed
            }
            KeyCode::PageDown if event.modifiers == KeyModifiers::NONE => {
                self.page_down(visible_height.max(1)); PopupOutcome::Consumed
            }
            KeyCode::Char(c)
                if event.modifiers == KeyModifiers::NONE
                    || event.modifiers == KeyModifiers::SHIFT =>
            {
                PopupOutcome::InsertChar(c)
            }
            KeyCode::Backspace if event.modifiers == KeyModifiers::NONE => PopupOutcome::Backspace,
            _ => PopupOutcome::Passthrough,
        }
    }
}

/// Whether the popup floats above or below its anchor row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PopupDirection {
    /// Render above the anchor; fall back to below when space is insufficient.
    Above,
    /// Always render below the anchor; cap height to available space below.
    Below,
}

/// Renders a `PopupListState` as a bordered, scrollable list anchored relative to
/// `(anchor_x, anchor_y)`.  `area` is the full terminal area used to clamp coordinates.
/// Returns the drawn `Rect` (or `Rect::default()` if nothing was rendered).
pub struct PopupListWidget<'a> {
    pub cs: &'a ColorScheme,
    pub state: &'a PopupListState,
    pub title: Option<&'a str>,
    pub direction: PopupDirection,
}

impl<'a> PopupListWidget<'a> {
    /// Returns `(popup_rect, scroll_offset)`. `initial_offset` is the offset
    /// from the previous frame; Ratatui keeps it when the selected item is
    /// already visible and only scrolls when it is not (e.g. first appearance
    /// or keyboard navigation past the visible window).
    pub fn render_at(
        &self,
        area: Rect,
        buf: &mut Buffer,
        anchor_x: u16,
        anchor_y: u16,
        initial_offset: usize,
    ) -> (Rect, usize) {
        let n = self.state.items.len();
        if n == 0 || area.width == 0 || area.height == 0 {
            return (Rect::default(), 0);
        }

        let max_len = self.state.items.iter().map(|s| s.chars().count()).max().unwrap_or(0);
        // +2 for left/right border
        let popup_width = ((max_len + 2) as u16).max(10).min(area.width);
        // +2 for top/bottom border; cap at 15 rows
        let desired_height = (n as u16 + 2).min(15);

        let (popup_height, popup_y) = match self.direction {
            PopupDirection::Below => {
                let space = area.height.saturating_sub(anchor_y + 1);
                if space < 3 {
                    return (Rect::default(), 0);
                }
                (desired_height.min(space), anchor_y + 1)
            }
            PopupDirection::Above => {
                let popup_y = anchor_y.saturating_sub(desired_height);
                let h = anchor_y - popup_y;
                if h < 3 {
                    return (Rect::default(), 0);
                }
                (h, popup_y)
            }
        };

        // Move left if popup would overflow the right edge
        let popup_x = anchor_x.min(area.width.saturating_sub(popup_width));

        let popup_area = Rect { x: popup_x, y: popup_y, width: popup_width, height: popup_height };

        Widget::render(Clear, popup_area, buf);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(to_color(self.cs.dialog_border_fg)))
            .style(Style::default().bg(to_color(self.cs.dialog_bg)));
        if let Some(t) = self.title {
            block = block.title(
                Title::from(format!(" {t} "))
                    .alignment(Alignment::Center),
            );
        }

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        let inner_w = inner.width as usize;

        let items: Vec<ListItem> = self
            .state
            .items
            .iter()
            .enumerate()
            .map(|(i, s)| {
                // Truncate long entries: replace the last visible char with '…'
                let display = if s.chars().count() > inner_w && inner_w > 1 {
                    let truncated: String = s.chars().take(inner_w - 1).collect();
                    format!("{truncated}\u{2026}")
                } else {
                    s.clone()
                };

                let style = if i == self.state.selected {
                    Style::default()
                        .fg(to_color(self.cs.selected_fg))
                        .bg(to_color(self.cs.selected_bg))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(to_color(self.cs.dialog_fg))
                        .bg(to_color(self.cs.dialog_bg))
                };
                ListItem::new(Line::from(Span::styled(display, style)))
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.state.selected));
        *list_state.offset_mut() = initial_offset;

        let list = List::new(items).highlight_style(
            Style::default()
                .fg(to_color(self.cs.selected_fg))
                .bg(to_color(self.cs.selected_bg))
                .add_modifier(Modifier::BOLD),
        );

        StatefulWidget::render(list, inner, buf, &mut list_state);

        (popup_area, list_state.offset())
    }
}
