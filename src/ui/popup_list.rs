use crate::config::ColorScheme;
use crate::ui::modal_event::PopupOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, StatefulWidget, Widget},
};

use super::to_color;

/// Generic selectable list state used by both completion and reverse-i-search popups.
pub struct PopupListState {
    pub items: Vec<String>,
    pub selected: usize,
    /// Optional right-aligned hint column (e.g. a keyboard shortcut), drawn dimmer than
    /// the item text. Either empty (no column) or parallel to `items`.
    pub hints: Vec<String>,
}

impl PopupListState {
    pub fn new(items: Vec<String>) -> Self {
        PopupListState { items, selected: 0, hints: Vec::new() }
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

    /// Removes the currently selected item, clamping `selected` to stay in bounds.
    /// Returns the removed item, if any.
    pub fn remove_selected(&mut self) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        if self.selected < self.hints.len() {
            self.hints.remove(self.selected);
        }
        let removed = self.items.remove(self.selected);
        if self.selected >= self.items.len() && self.selected > 0 {
            self.selected -= 1;
        }
        Some(removed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_selected_drops_matching_hint() {
        let mut s = PopupListState::new(vec!["a".into(), "b".into()]);
        s.hints = vec!["x".into(), "y".into()];
        s.selected = 0;
        s.remove_selected();
        assert_eq!(s.hints, vec!["y"]);
    }

    #[test]
    fn hint_is_right_aligned_in_its_own_style() {
        let cs = ColorScheme::default();
        let mut state = PopupListState::new(vec!["Copy".into(), "Quit".into()]);
        state.hints = vec!["F5".into(), "F10".into()];
        state.selected = 1;
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        let w = PopupListWidget { cs: &cs, state: &state, title: None, direction: PopupDirection::Below, fixed_width: None, title_left: false };
        let (r, _) = w.render_at(area, &mut buf, 0, 0, 0);
        let row = |y: u16| -> String { (r.x..r.x + r.width).map(|x| buf[(x, y)].symbol().to_string()).collect() };
        let first = row(r.y + 1);
        assert!(first.contains("Copy") && first.trim_end_matches('│').trim_end().ends_with("F5"), "{first:?}");
        assert!(buf[(r.x + r.width - 2, r.y + 1)].modifier.contains(Modifier::DIM));
        assert!(!buf[(r.x + r.width - 2, r.y + 2)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn remove_selected_on_empty_list_is_noop() {
        let mut s = PopupListState::new(vec![]);
        assert_eq!(s.remove_selected(), None);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn remove_selected_middle_keeps_index() {
        let mut s = PopupListState::new(vec!["a".into(), "b".into(), "c".into()]);
        s.selected = 1;
        assert_eq!(s.remove_selected(), Some("b".to_string()));
        assert_eq!(s.items, vec!["a", "c"]);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn remove_selected_last_item_clamps_index() {
        let mut s = PopupListState::new(vec!["a".into(), "b".into(), "c".into()]);
        s.selected = 2;
        assert_eq!(s.remove_selected(), Some("c".to_string()));
        assert_eq!(s.items, vec!["a", "b"]);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn remove_selected_only_item_leaves_empty_list() {
        let mut s = PopupListState::new(vec!["a".into()]);
        assert_eq!(s.remove_selected(), Some("a".to_string()));
        assert!(s.items.is_empty());
        assert_eq!(s.selected, 0);
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
    /// When set, the popup is always this wide (capped at the terminal width) instead of
    /// fitting its content.
    pub fixed_width: Option<u16>,
    /// Left-align the title instead of centring it.
    pub title_left: bool,
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
        let max_hint = self.state.hints.iter().map(|s| s.chars().count()).max().unwrap_or(0);
        // Two spaces separate the text column from the hint column.
        let content_len = if max_hint > 0 { max_len + 2 + max_hint } else { max_len };
        let title_len = self.title.map_or(0, |t| t.chars().count() + 4);
        // +2 for left/right border
        let popup_width = match self.fixed_width {
            Some(w) => w.min(area.width),
            None => ((content_len.max(title_len) + 2) as u16).max(10).min(area.width),
        };
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
            let line = Line::from(format!(" {t} "));
            block = block.title_top(if self.title_left { line.left_aligned() } else { line.centered() });
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
                let selected = i == self.state.selected;
                let style = if selected {
                    Style::default()
                        .fg(to_color(self.cs.selected_fg))
                        .bg(to_color(self.cs.selected_bg))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(to_color(self.cs.dialog_fg))
                        .bg(to_color(self.cs.dialog_bg))
                };
                let hint = self.state.hints.get(i).map(String::as_str).unwrap_or("");
                let hint_len = hint.chars().count();
                // The hint keeps its full width; the text is truncated to make room.
                let text_room = if hint_len > 0 { inner_w.saturating_sub(hint_len + 2) } else { inner_w };
                let text_len = s.chars().count();
                let display = if text_len > text_room && text_room > 1 {
                    let truncated: String = s.chars().take(text_room - 1).collect();
                    format!("{truncated}\u{2026}")
                } else {
                    s.clone()
                };
                if hint_len == 0 {
                    return ListItem::new(Line::from(Span::styled(display, style)));
                }
                let used = display.chars().count();
                let pad = " ".repeat(inner_w.saturating_sub(used + hint_len));
                let hint_style = if selected {
                    style.remove_modifier(Modifier::BOLD)
                } else {
                    style.add_modifier(Modifier::DIM)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(display, style),
                    Span::styled(pad, style),
                    Span::styled(hint.to_string(), hint_style),
                ]))
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
