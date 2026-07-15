use crate::config::{ActionBindings, ColorScheme, bindings_match_event};
use crate::pattern::find_matches;
use crate::ui::modal_event::OverlayOutcome;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget, Wrap,
    },
};

use super::to_color;

pub struct OutputOverlayState {
    pub scroll: u16,
}

impl OutputOverlayState {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }

    /// Open the view scrolled so the given 1-based line is at the top,
    /// with a couple of lines of context above it when possible.
    pub fn jump_to_line(&mut self, line: u64) {
        self.scroll = line.saturating_sub(3).min(u16::MAX as u64) as u16;
    }

    pub fn handle_key(&mut self, event: &KeyEvent, dismiss_bindings: &ActionBindings) -> OverlayOutcome {
        if event.code == KeyCode::Esc || bindings_match_event(dismiss_bindings, event) {
            return OverlayOutcome::Dismissed;
        }
        match event.code {
            KeyCode::Up => { self.scroll = self.scroll.saturating_sub(1); OverlayOutcome::Consumed }
            KeyCode::Down => { self.scroll = self.scroll.saturating_add(1); OverlayOutcome::Consumed }
            KeyCode::PageUp => { self.scroll = self.scroll.saturating_sub(20); OverlayOutcome::Consumed }
            KeyCode::PageDown => { self.scroll = self.scroll.saturating_add(20); OverlayOutcome::Consumed }
            _ => OverlayOutcome::Passthrough,
        }
    }

    pub fn scroll_by(&mut self, delta: i16) {
        if delta < 0 {
            self.scroll = self.scroll.saturating_sub((-delta) as u16);
        } else {
            self.scroll = self.scroll.saturating_add(delta as u16);
        }
    }

    pub fn scrollbar_click(&mut self, track_row: usize, inner_h: usize, total_lines: usize) {
        if let Some(pos) = (track_row * total_lines).checked_div(inner_h) {
            self.scroll = pos as u16;
        }
    }
}

/// Full-screen text viewer: shows either the last command's output or a file
/// from disk, optionally highlighting search matches.
pub struct OutputOverlayWidget<'a> {
    pub cs: &'a ColorScheme,
    pub text: &'a str,
    pub scroll: u16,
    pub title: &'a str,
    /// `(needle, case_sensitive)` — occurrences get the search-match colors.
    pub highlight: Option<(&'a str, bool)>,
}

impl<'a> Widget for OutputOverlayWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(Clear, area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(to_color(self.cs.dialog_border_fg)))
            .title(Span::from(self.title))
            .style(Style::default().bg(to_color(self.cs.panel_bg)));

        let inner = block.inner(area);
        block.render(area, buf);

        let style = Style::default()
            .fg(to_color(self.cs.panel_fg))
            .bg(to_color(self.cs.panel_bg));

        let total_lines = self.text.lines().count();

        let text_area = Rect { width: inner.width.saturating_sub(1), ..inner };

        let para = match self.highlight {
            Some((needle, case_sensitive)) if !needle.is_empty() => {
                let match_style = Style::default()
                    .fg(to_color(self.cs.search_match_fg))
                    .bg(to_color(self.cs.search_match_bg));
                let lines: Vec<Line> = self
                    .text
                    .lines()
                    .map(|line| {
                        let mut spans = Vec::new();
                        let mut pos = 0;
                        for (start, end) in find_matches(line, needle, case_sensitive) {
                            if start > pos {
                                spans.push(Span::styled(&line[pos..start], style));
                            }
                            spans.push(Span::styled(&line[start..end], match_style));
                            pos = end;
                        }
                        if pos < line.len() {
                            spans.push(Span::styled(&line[pos..], style));
                        }
                        Line::from(spans)
                    })
                    .collect();
                Paragraph::new(lines)
            }
            _ => Paragraph::new(self.text),
        };
        let para = para
            .style(style)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        Widget::render(para, text_area, buf);

        let scrollbar_area = Rect {
            x: inner.x + inner.width.saturating_sub(1),
            width: 1,
            ..inner
        };
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(self.scroll as usize);
        StatefulWidget::render(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            scrollbar_area,
            buf,
            &mut scrollbar_state,
        );
    }
}
