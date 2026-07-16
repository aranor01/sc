use crate::config::{ActionBindings, ColorScheme, bindings_match_event};
use crate::pattern::ContentMatcher;
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
    /// Set by `jump_to_line`, resolved into an actual `scroll` by
    /// `resolve_pending_jump` once the render width is known — wrapping is
    /// width-dependent, and `jump_to_line` is called before that width is
    /// available (from key handling, not from the render pass).
    pending_line: Option<u64>,
}

impl OutputOverlayState {
    pub fn new() -> Self {
        Self { scroll: 0, pending_line: None }
    }

    /// Request the view be scrolled so the given 1-based source line ends up a
    /// couple of rows below the top when possible. Takes effect on the next
    /// `resolve_pending_jump` call (from the render pass), since converting a raw
    /// line number into a scroll offset requires knowing the wrap width.
    pub fn jump_to_line(&mut self, line: u64) {
        self.pending_line = Some(line);
    }

    /// Converts a pending `jump_to_line` request into an actual wrap-aware
    /// `scroll` value, now that `width` (the content area's column count) is
    /// known. `highlight` (same shape as `OutputOverlayWidget::highlight`) is
    /// used to find where in the target line the actual match sits, so a match
    /// deep inside a long wrapped line lands in view instead of just the start
    /// of its raw line. No-op if there's no pending request.
    pub fn resolve_pending_jump(&mut self, text: &str, width: u16, highlight: Option<(&str, bool, bool, bool)>) {
        let Some(line) = self.pending_line.take() else { return };
        let width = width as usize;
        let matcher = highlight
            .filter(|(needle, ..)| !needle.is_empty())
            .and_then(|(needle, case_sensitive, is_regex, whole_words)| {
                ContentMatcher::build(needle, is_regex, case_sensitive, whole_words).ok()
            });

        let mut rows_before = 0u64;
        for (idx, raw_line) in text.lines().enumerate() {
            if idx as u64 + 1 == line {
                if let Some((m_start, _)) =
                    matcher.as_ref().and_then(|m| m.find_matches(raw_line).into_iter().next())
                {
                    rows_before += (wrapped_row_count(&raw_line[..m_start], width) - 1) as u64;
                }
                break;
            }
            rows_before += wrapped_row_count(raw_line, width) as u64;
        }
        self.scroll = rows_before.saturating_sub(2).min(u16::MAX as u64) as u16;
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

/// Width available for wrapped text inside the overlay: `area` minus the 2
/// border columns and the 1 scrollbar column reserved by `render`. Exposed so
/// callers can keep `resolve_pending_jump`'s wrap math in sync with what
/// `render` will actually lay out.
pub fn content_width(area: Rect) -> u16 {
    area.width.saturating_sub(3)
}

/// Number of display rows `line` occupies when word-wrapped to `width` columns:
/// words (split on whitespace) are packed greedily onto a row, and a single word
/// longer than `width` is hard-broken across multiple rows — mirroring ratatui's
/// own `Wrap` word-wrapping closely enough to convert a source line number into a
/// scroll offset. Char-count based (like the rest of this codebase), not full
/// Unicode display-width aware.
fn wrapped_row_count(line: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut rows = 1usize;
    let mut col = 0usize;
    for word in line.split_whitespace() {
        let word_len = word.chars().count();
        let sep = if col == 0 { 0 } else { 1 };
        if col + sep + word_len <= width {
            col += sep + word_len;
        } else {
            if col > 0 {
                rows += 1;
            }
            if word_len <= width {
                col = word_len;
            } else {
                let extra_rows = word_len.saturating_sub(1) / width;
                rows += extra_rows;
                col = word_len - extra_rows * width;
            }
        }
    }
    rows
}

/// Full-screen text viewer: shows either the last command's output or a file
/// from disk, optionally highlighting search matches.
pub struct OutputOverlayWidget<'a> {
    pub cs: &'a ColorScheme,
    pub text: &'a str,
    pub scroll: u16,
    pub title: &'a str,
    /// `(needle, case_sensitive, is_regex, whole_words)` — occurrences get the
    /// search-match colors.
    pub highlight: Option<(&'a str, bool, bool, bool)>,
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

        let text_area = Rect { width: content_width(area), ..inner };

        let para = match self.highlight {
            Some((needle, case_sensitive, is_regex, whole_words)) if !needle.is_empty() => {
                let match_style = Style::default()
                    .fg(to_color(self.cs.search_match_fg))
                    .bg(to_color(self.cs.search_match_bg));
                let matcher = ContentMatcher::build(needle, is_regex, case_sensitive, whole_words).ok();
                let lines: Vec<Line> = self
                    .text
                    .lines()
                    .map(|line| {
                        let mut spans = Vec::new();
                        let mut pos = 0;
                        let hits = matcher.as_ref().map(|m| m.find_matches(line)).unwrap_or_default();
                        for (start, end) in hits {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to_buffer(text: &str, scroll: u16, area: Rect) -> Buffer {
        let cs = ColorScheme::default();
        let widget = OutputOverlayWidget { cs: &cs, text, scroll, title: "test", highlight: None };
        let mut buf = Buffer::empty(area);
        Widget::render(widget, area, &mut buf);
        buf
    }

    fn row_text(buf: &Buffer, y: u16, area: Rect) -> String {
        (area.x..area.x + area.width)
            .map(|x| buf.cell((x, y)).map(|c| c.symbol().to_string()).unwrap_or_default())
            .collect()
    }

    fn find_row_containing(buf: &Buffer, area: Rect, needle: &str) -> Option<u16> {
        (area.y..area.y + area.height).find(|&y| row_text(buf, y, area).contains(needle))
    }

    #[test]
    fn wrapped_row_count_fits_on_one_row() {
        assert_eq!(wrapped_row_count("hello world", 20), 1);
    }

    #[test]
    fn wrapped_row_count_wraps_on_word_boundary() {
        assert_eq!(wrapped_row_count("hello world", 8), 2);
    }

    #[test]
    fn wrapped_row_count_hard_breaks_an_overlong_word() {
        assert_eq!(wrapped_row_count(&"A".repeat(25), 10), 3);
    }

    #[test]
    fn wrapped_row_count_empty_line_is_one_row() {
        assert_eq!(wrapped_row_count("", 10), 1);
    }

    /// Cross-checks `wrapped_row_count`'s predictions against ratatui's actual
    /// `Wrap{trim:false}` rendering, so the jump-target math is validated against
    /// real behavior rather than just against itself.
    #[test]
    fn wrapped_row_count_matches_actual_ratatui_wrapping() {
        let lines = ["short1", &"A".repeat(60), "short2", "needle-here"];
        let text = format!("{}\n", lines.join("\n"));
        let area = Rect::new(0, 0, 16, 30);
        let buf = render_to_buffer(&text, 0, area);
        let content_w = content_width(area) as usize;

        let predicted_rows_before: usize =
            lines[..3].iter().map(|l| wrapped_row_count(l, content_w)).sum();

        let target_row = find_row_containing(&buf, area, "needle-here")
            .expect("target line must be rendered somewhere");
        let content_row = (target_row - (area.y + 1)) as usize;
        assert_eq!(content_row, predicted_rows_before);
    }

    #[test]
    fn jump_to_line_keeps_target_visible_despite_earlier_wrapping() {
        let lines = ["short1", &"A".repeat(200), "short2", "needle-here"];
        let text = format!("{}\n", lines.join("\n"));
        // Narrow width forces the long line to wrap into many rows, and a short
        // viewport means a naive raw-line-count scroll would leave line 4 far
        // below the visible window (the bug this test guards against).
        let area = Rect::new(0, 0, 16, 8);

        let mut state = OutputOverlayState::new();
        state.jump_to_line(4);
        state.resolve_pending_jump(&text, content_width(area), None);

        let buf = render_to_buffer(&text, state.scroll, area);
        assert!(
            find_row_containing(&buf, area, "needle-here").is_some(),
            "target line must be visible after jump_to_line despite earlier wrapping"
        );
    }

    /// A match deep inside a single very long wrapped line (e.g. a build `.d`
    /// file with one giant space-separated line) must itself end up visible —
    /// jumping to the start of that raw line isn't enough when the match is
    /// dozens of wrapped rows into it.
    #[test]
    fn jump_to_line_finds_a_match_deep_inside_a_long_wrapped_line() {
        let words: Vec<String> = (0..40).map(|i| format!("filler-word-{i:03}-padded")).collect();
        let mut long_line = words.join(" ");
        long_line.push_str(" needle-token more-filler-after-the-match-token-here");
        let text = format!("intro\n{long_line}\n");
        let area = Rect::new(0, 0, 20, 8);

        let mut state = OutputOverlayState::new();
        state.jump_to_line(2);
        state.resolve_pending_jump(&text, content_width(area), Some(("needle-token", true, false, false)));

        let buf = render_to_buffer(&text, state.scroll, area);
        assert!(
            find_row_containing(&buf, area, "needle-token").is_some(),
            "the match itself must be visible, not just the start of its (long) raw line"
        );
    }

    #[test]
    fn resolve_pending_jump_is_a_noop_without_a_pending_line() {
        let mut state = OutputOverlayState::new();
        state.scroll = 5;
        state.resolve_pending_jump("a\nb\nc\n", 40, None);
        assert_eq!(state.scroll, 5);
    }
}
