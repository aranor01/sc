use crate::config::{ActionBindings, ColorScheme, bindings_match_event};
use crate::pattern::ContentMatcher;
use crate::ui::modal_event::OverlayOutcome;
use crossterm::event::{KeyCode, KeyEvent};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
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

/// Builds a `ContentMatcher` from a `(needle, case_sensitive, is_regex,
/// whole_words)` highlight tuple, the same shape used by
/// `OutputOverlayWidget::highlight`. Returns `None` for an empty needle or an
/// invalid pattern (e.g. a bad regex).
fn build_matcher(highlight: Option<(&str, bool, bool, bool)>) -> Option<ContentMatcher> {
    let (needle, case_sensitive, is_regex, whole_words) = highlight.filter(|(needle, ..)| !needle.is_empty())?;
    ContentMatcher::build(needle, is_regex, case_sensitive, whole_words).ok()
}

pub struct OutputOverlayState {
    pub scroll: u16,
    /// Set by `jump_to_line`, resolved into an actual `scroll` by
    /// `resolve_pending_jump` once the render width is known — wrapping is
    /// width-dependent, and `jump_to_line` is called before that width is
    /// available (from key handling, not from the render pass).
    pending_line: Option<u64>,
    /// Set after whan scrolling downward and by refresh, consumed by
    /// `cap_scroll` in the render cycle.
    pending_cap_scroll: bool,
    /// Total wrapped display rows the text occupies at the last render
    /// width. Computed first time is needed by `resolve_pending_end`.
    total_rows: Option<u64>,
}

impl OutputOverlayState {
    pub fn new() -> Self {
        Self { scroll: 0, pending_line: None, pending_cap_scroll: false, total_rows: None }
    }

    /// Resets scroll to the top and drops any unresolved `jump_to_line` or
    /// `End` request. Called whenever the viewer starts showing different
    /// content, so a new file (or command output) never inherits the
    /// previous one's scroll position.
    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.pending_line = None;
        self.pending_cap_scroll = false;
        self.total_rows = None;
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
    /// of its raw line. No-op if there's no pending request. If `text` no
    /// longer has that many lines (e.g. the file was edited between when a
    /// search hit recorded the line and this jump), leaves `scroll` untouched
    /// rather than landing near the end of the file.
    pub fn resolve_pending_jump(&mut self, text: &str, width: u16, highlight: Option<(&str, bool, bool, bool)>) {
        let Some(line) = self.pending_line.take() else { return };
        let width = width as usize;
        let matcher = build_matcher(highlight);

        let mut rows_before = 0u64;
        let mut found = false;
        for (idx, raw_line) in text.lines().enumerate() {
            // Match render()'s tab expansion so the predicted row lines up
            // with what's actually displayed.
            let line_text = super::expand_tabs(raw_line);
            if idx as u64 + 1 == line {
                found = true;
                if let Some((m_start, _)) =
                    matcher.as_ref().and_then(|m| m.find_matches(&line_text).into_iter().next())
                {
                    rows_before += (wrapped_row_count(&line_text[..m_start], width) - 1) as u64;
                }
                break;
            }
            rows_before += wrapped_row_count(&line_text, width) as u64;
        }
        if found {
            self.scroll = rows_before.saturating_sub(2).min(u16::MAX as u64) as u16;
        }
    }

    /// Recomputes and caches the total wrapped-row count for `text` at `width`,
    /// when it's needed to calculate the upper limit for `scroll``
    /// Safe to call unconditionally from `render`; it's a no-op on `scroll`
    /// when nothing is pending.
    pub fn cap_scroll(&mut self, text: &str, width: u16, viewport_height: u16) {
        if !self.pending_cap_scroll {
            return;
        }
        self.pending_cap_scroll = false;

        // Cheap upper-bound check: wrapped row count is always >= raw line count,
        // so if we're within the limit we don't call wrapped_row_count
        let scroll_limit = text.lines().count().saturating_sub(viewport_height as usize).min(u16::MAX as usize) as u16;

        if self.scroll <= scroll_limit
        {
            return;
        }

        let width = width as usize;
        // Match render()'s tab expansion so the row count lines up with what's
        // actually displayed.
        let total_rows = self.total_rows.get_or_insert_with(|| {
            text.lines()
            .map(|raw_line| wrapped_row_count(&super::expand_tabs(raw_line), width) as u64)
            .sum()
        });

        let scroll_limit = total_rows.saturating_sub(viewport_height as u64).min(u16::MAX as u64) as u16;

        if self.scroll > scroll_limit
        {
            self.scroll = scroll_limit;
        }
    }

    pub fn handle_key(&mut self, event: &KeyEvent, dismiss_bindings: &ActionBindings) -> OverlayOutcome {
        if event.code == KeyCode::Esc || bindings_match_event(dismiss_bindings, event) {
            return OverlayOutcome::Dismissed;
        }
        match event.code {
            KeyCode::Up => { self.scroll = self.scroll.saturating_sub(1); OverlayOutcome::Consumed }
            KeyCode::Down => { self.scroll = self.scroll.saturating_add(1); self.pending_cap_scroll = true; OverlayOutcome::Consumed }
            KeyCode::PageUp => { self.scroll = self.scroll.saturating_sub(20); OverlayOutcome::Consumed }
            KeyCode::PageDown => { self.scroll = self.scroll.saturating_add(20); self.pending_cap_scroll = true; OverlayOutcome::Consumed }
            KeyCode::Home => { self.scroll = 0u16; OverlayOutcome::Consumed }
            KeyCode::End => { self.scroll = u16::MAX; self.pending_cap_scroll = true; OverlayOutcome::Consumed }
            _ => OverlayOutcome::Passthrough,
        }
    }

    pub fn scroll_by(&mut self, delta: i16) {
        if delta < 0 {
            self.scroll = self.scroll.saturating_sub((-delta) as u16);
        } else {
            self.scroll = self.scroll.saturating_add(delta as u16);
            self.pending_cap_scroll = true;
        }
    }

    pub fn scrollbar_click(&mut self, track_row: usize, inner_h: usize, total_lines: usize) {
        if let Some(pos) = (track_row * total_lines).checked_div(inner_h) {
            if pos > (self.scroll as usize) {
                self.pending_cap_scroll = true;
            }
            self.scroll = pos as u16;
        }
    }

    pub fn refresh(&mut self) {
        self.pending_cap_scroll = true;
    }

}

/// Width available for wrapped text inside the overlay: `area` minus the 2
/// border columns and the 1 scrollbar column reserved by `render`. Exposed so
/// callers can keep `resolve_pending_jump`'s wrap math in sync with what
/// `render` will actually lay out.
pub fn content_width(area: Rect) -> u16 {
    area.width.saturating_sub(3)
}

/// Splits `line` into maximal runs of whitespace / non-whitespace characters,
/// in order, keeping every character (unlike `split_whitespace`, which drops
/// whitespace runs entirely) — needed because ratatui's `Wrap{trim:false}`
/// lays out whitespace verbatim rather than collapsing or trimming it. Each
/// segment is paired with whether it's a whitespace run.
fn whitespace_runs(line: &str) -> Vec<(bool, &str)> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut current_is_ws: Option<bool> = None;
    for (i, c) in line.char_indices() {
        let is_ws = c.is_whitespace();
        match current_is_ws {
            None => current_is_ws = Some(is_ws),
            Some(prev) if prev != is_ws => {
                segments.push((prev, &line[start..i]));
                start = i;
                current_is_ws = Some(is_ws);
            }
            _ => {}
        }
    }
    if let Some(is_ws) = current_is_ws {
        segments.push((is_ws, &line[start..]));
    }
    segments
}

/// Number of display rows `line` occupies when wrapped to `width` columns:
/// whitespace and non-whitespace runs are packed greedily onto a row (using
/// Unicode display width, matching what a terminal actually renders), and a
/// single run longer than `width` is hard-broken across multiple rows —
/// mirroring ratatui's own `Wrap{trim:false}` word-wrapping closely enough to
/// convert a source line number into a scroll offset. A whitespace run that's
/// hard-broken drops the one character that triggers each wrap point (ratatui
/// elides exactly that character even with `trim:false`); a non-whitespace
/// run doesn't.
// replace with ratatui method in the future
// let lines: Vec<Line> = text
//     .lines()
//     .map(|raw_line| Line::from(super::expand_tabs(raw_line)))
//     .collect();
// Paragraph::new(lines)
//     .wrap(Wrap { trim: false })
//     .line_count(width) as u64;
fn wrapped_row_count(line: &str, width: usize) -> usize {
    let width = width.max(1);
    if line.is_empty() {
        return 1;
    }
    let mut rows = 1usize;
    let mut col = 0usize;
    for (is_ws, seg) in whitespace_runs(line) {
        let seg_w = seg.width();
        if col > 0 && col + seg_w <= width {
            col += seg_w;
            continue;
        }
        if col > 0 {
            rows += 1;
            col = 0;
        }
        if seg_w <= width {
            col = seg_w;
        } else {
            // Hard-break this run across rows, one column at a time, since
            // its graphemes may have mixed display widths.
            for ch in seg.chars() {
                let ch_w = ch.width().unwrap_or(0);
                if col + ch_w > width && col > 0 {
                    rows += 1;
                    col = 0;
                    if is_ws {
                        continue; // this whitespace char is the elided wrap point
                    }
                }
                col += ch_w;
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
    pub title: &'a str,
    /// `(needle, case_sensitive, is_regex, whole_words)` — occurrences get the
    /// search-match colors.
    pub highlight: Option<(&'a str, bool, bool, bool)>,
}

impl<'a> StatefulWidget for OutputOverlayWidget<'a> {
    type State = OutputOverlayState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
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

        let total_lines = state.total_rows.unwrap_or(self.text.lines().count() as u64) as usize;

        let text_area = Rect { width: content_width(area), ..inner };
        state.cap_scroll(self.text, text_area.width, inner.height);

        state.resolve_pending_jump(self.text, text_area.width, self.highlight);
        

        let matcher = build_matcher(self.highlight);
        let match_style = Style::default()
            .fg(to_color(self.cs.search_match_fg))
            .bg(to_color(self.cs.search_match_bg));
        // Tabs must be expanded before matching/rendering: a raw tab written
        // to the terminal jumps the physical cursor past what ratatui's
        // buffer model expects, breaking the overlay's own border on that row.
        let lines: Vec<Line> = self
            .text
            .lines()
            .map(|raw_line| {
                let line = super::expand_tabs(raw_line);
                let Some(m) = matcher.as_ref() else {
                    return Line::from(Span::styled(line, style));
                };
                let mut spans = Vec::new();
                let mut pos = 0;
                for (start, end) in m.find_matches(&line) {
                    if start > pos {
                        spans.push(Span::styled(line[pos..start].to_string(), style));
                    }
                    spans.push(Span::styled(line[start..end].to_string(), match_style));
                    pos = end;
                }
                if pos < line.len() {
                    spans.push(Span::styled(line[pos..].to_string(), style));
                }
                Line::from(spans)
            })
            .collect();
        let para = Paragraph::new(lines);
        let para = para
            .style(style)
            .wrap(Wrap { trim: false })
            .scroll((state.scroll, 0));
        Widget::render(para, text_area, buf);

        let scrollbar_area = Rect {
            x: inner.x + inner.width.saturating_sub(1),
            width: 1,
            ..inner
        };
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(state.scroll as usize).viewport_content_length(inner.height as usize);
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

    /// Same cross-check as above, but for a line with heavy leading
    /// whitespace before a short word — `split_whitespace`-based counting
    /// would drop the whitespace entirely and undercount rows.
    #[test]
    fn wrapped_row_count_matches_actual_ratatui_wrapping_with_leading_whitespace() {
        let indented = format!("{}X", " ".repeat(80));
        let lines = ["short1", indented.as_str(), "short2", "needle-here"];
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

    /// Same cross-check, but with wide (CJK) characters — char-count-based
    /// width would undercount rows since ratatui uses display width.
    #[test]
    fn wrapped_row_count_matches_actual_ratatui_wrapping_with_wide_chars() {
        let wide_line = "\u{6f22}\u{5b57}".repeat(20); // 40 wide chars, 80 display columns
        let lines = ["short1", wide_line.as_str(), "short2", "needle-here"];
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

    /// If the text no longer has as many lines as the requested jump target
    /// (e.g. the file shrank between when a search hit recorded a line and
    /// when the viewer opened it), the scroll must be left alone rather than
    /// silently landing near the end of the file.
    #[test]
    fn resolve_pending_jump_leaves_scroll_untouched_for_an_out_of_range_line() {
        let mut state = OutputOverlayState::new();
        state.scroll = 5;
        state.jump_to_line(100);
        state.resolve_pending_jump("a\nb\nc\n", 40, None);
        assert_eq!(state.scroll, 5);
    }

    #[test]
    fn build_matcher_is_none_for_an_empty_needle() {
        assert!(build_matcher(Some(("", true, false, false))).is_none());
        assert!(build_matcher(None).is_none());
    }

    #[test]
    fn build_matcher_is_some_for_a_valid_needle() {
        assert!(build_matcher(Some(("needle", true, false, false))).is_some());
    }

    /// A raw tab written straight to the terminal jumps the physical cursor
    /// past what ratatui expects, breaking the overlay's own border on that
    /// row — tabs must be expanded to spaces before rendering.
    #[test]
    fn viewer_expands_tabs_before_rendering() {
        let text = "before\ttab\tneedle\n";
        let area = Rect::new(0, 0, 40, 10);
        let buf = render_to_buffer(text, 0, area);

        let row = find_row_containing(&buf, area, "needle").expect("line must be rendered");
        let content = row_text(&buf, row, area);
        assert!(!content.contains('\t'), "raw tabs must be expanded before rendering: {content:?}");

        // The right border column must still be the border, not overrun content.
        let border_col = area.x + area.width - 1;
        let border_row = area.y + 2; // a plain vertical-bar border row, below the corner
        let border_symbol = buf.cell((border_col, border_row)).unwrap().symbol().to_string();
        let content_border = buf.cell((border_col, row)).unwrap().symbol().to_string();
        assert_eq!(
            content_border, border_symbol,
            "tab-containing row must not overwrite the border: {content_border:?} vs {border_symbol:?}"
        );
    }
}
