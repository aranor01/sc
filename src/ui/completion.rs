use crate::config::ColorScheme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, StatefulWidget, Widget},
};

use super::to_color;

/// State for the completion popup (candidates + selected index + anchor).
pub struct CompletionPopup {
    pub candidates: Vec<String>,
    pub selected: usize,
    /// Byte offset in cmdline.text marking the start of the word being completed.
    pub word_start: usize,
}

impl CompletionPopup {
    pub fn new(candidates: Vec<String>, word_start: usize) -> Self {
        CompletionPopup { candidates, selected: 0, word_start }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if !self.candidates.is_empty() && self.selected + 1 < self.candidates.len() {
            self.selected += 1;
        }
    }

    pub fn move_top(&mut self) {
        self.selected = 0;
    }

    pub fn move_bottom(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = self.candidates.len() - 1;
        }
    }

    pub fn page_up(&mut self, page: usize) {
        self.selected = self.selected.saturating_sub(page);
    }

    pub fn page_down(&mut self, page: usize) {
        if !self.candidates.is_empty() {
            self.selected = (self.selected + page).min(self.candidates.len() - 1);
        }
    }

    pub fn selected_candidate(&self) -> Option<&str> {
        self.candidates.get(self.selected).map(String::as_str)
    }
}

/// Renders `popup` as a bordered list anchored just above (`anchor_x`, `anchor_y`).
/// `area` is the full terminal area used to clamp coordinates.
/// Returns the actual Rect drawn (Rect::default() if nothing was rendered).
pub struct CompletionWidget<'a> {
    pub cs: &'a ColorScheme,
    pub popup: &'a CompletionPopup,
}

impl<'a> CompletionWidget<'a> {
    pub fn render_at(
        &self,
        area: Rect,
        buf: &mut Buffer,
        anchor_x: u16,
        anchor_y: u16,
    ) -> Rect {
        let n = self.popup.candidates.len();
        // Need at least one row above anchor_y for the popup
        if n == 0 || anchor_y == 0 || area.width == 0 {
            return Rect::default();
        }

        let max_len = self.popup.candidates.iter().map(|s| s.len()).max().unwrap_or(0);
        // +2 for left/right border
        let popup_width = ((max_len + 2) as u16).max(10).min(area.width);
        // +2 for top/bottom border; cap height at 15 rows or rows available above anchor
        let popup_height = (n as u16 + 2).min(15).min(anchor_y);

        if popup_height < 3 {
            return Rect::default(); // not enough space even for 1 item + borders
        }

        // Move left if popup would overflow the right edge
        let popup_x = anchor_x.min(area.width.saturating_sub(popup_width));
        // Popup sits immediately above anchor_y
        let popup_y = anchor_y - popup_height;

        let popup_area = Rect { x: popup_x, y: popup_y, width: popup_width, height: popup_height };

        Widget::render(Clear, popup_area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(to_color(self.cs.dialog_border_fg)))
            .style(Style::default().bg(to_color(self.cs.dialog_bg)));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        let items: Vec<ListItem> = self
            .popup
            .candidates
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let style = if i == self.popup.selected {
                    Style::default()
                        .fg(to_color(self.cs.selected_fg))
                        .bg(to_color(self.cs.selected_bg))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(to_color(self.cs.dialog_fg))
                        .bg(to_color(self.cs.dialog_bg))
                };
                ListItem::new(Line::from(Span::styled(s.as_str(), style)))
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.popup.selected));

        let list = List::new(items).highlight_style(
            Style::default()
                .fg(to_color(self.cs.selected_fg))
                .bg(to_color(self.cs.selected_bg))
                .add_modifier(Modifier::BOLD),
        );

        StatefulWidget::render(list, inner, buf, &mut list_state);

        popup_area
    }
}
