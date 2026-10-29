use crate::config::{ActionBindings, ColorScheme, bindings_match_event};
use crate::pattern::ContentMatcher;
use crate::textbuf::TextBuffer;
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

/// Direction of a text-viewer search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

pub struct OutputOverlayState {
    pub scroll: u64,
    pending_line: Option<u64>,
    pending_cap_scroll: bool,
    /// Wrap width `row_cache` was last built at. Checked against the render
    /// width on every use; a mismatch forces a rebuild, since every cached
    /// row count is a function of wrap width.
    row_cache_width: u16,
    /// `row_cache[i]` = wrapped rows occupied by raw lines `0..i`, i.e. the
    /// scroll offset at which raw line `i` begins (`row_cache[0] == 0`
    /// always). Grows lazily and monotonically: only ever extended as far
    /// as a render, jump, or cap-scroll has actually needed, and never
    /// re-walks a line it's already accounted for. This is what lets
    /// scrolling through a huge file expand tabs for each line once ever,
    /// rather than for the whole file every frame.
    row_cache: Vec<u64>,
    /// Set once `row_cache` has an entry for every line, at which point
    /// `row_cache.len() - 1` is the true line count and `row_cache.last()`
    /// the true total wrapped-row count — both previously found by
    /// rescanning `text` directly.
    row_cache_complete: bool,
    /// `TextBuffer::id` of the text `row_cache` describes.
    source_id: u64,
    /// Digits reserved for line numbers in the gutter. Only ever grows while a
    /// text is shown, so the wrap width doesn't change on every refined estimate.
    gutter_digits: u16,
    /// Raw line (0-based) shown at the top, as of the last render.
    pub top_line: usize,
    /// Line the last search jump landed on; the next/previous match continues from it
    /// until something moves the view.
    pub match_line: Option<usize>,
    /// Line holding the current search match, drawn with the `search_current_*` colors.
    /// Unlike `match_line` it survives scrolling; a new search or file resets it.
    pub current_line: Option<usize>,
    /// Feedback for the user, shown in the bottom border until the next key.
    pub message: Option<String>,
    /// Set after a search ran out of matches in this direction: pressing the same
    /// direction again wraps around.
    pub wrap_armed: Option<SearchDirection>,
}

impl OutputOverlayState {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            pending_line: None,
            pending_cap_scroll: false,
            row_cache_width: 0,
            row_cache: vec![0],
            row_cache_complete: false,
            source_id: 0,
            gutter_digits: 0,
            top_line: 0,
            match_line: None,
            current_line: None,
            message: None,
            wrap_armed: None,
        }
    }

    /// If `buf` is a different text from last time, the row cache described
    /// the old one and is dropped.
    fn sync_text(&mut self, buf: &TextBuffer) {
        if self.source_id != buf.id() {
            self.source_id = buf.id();
            self.gutter_digits = 0;
            self.reset_row_cache();
        }
    }

    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.pending_line = None;
        self.pending_cap_scroll = false;
        self.gutter_digits = 0;
        self.current_line = None;
        self.moved();
        self.reset_row_cache();
    }

    /// Request the view be scrolled so the given 1-based source line ends up a
    /// couple of rows below the top when possible. Takes effect on the next
    /// `resolve_pending_jump` call (from the render pass), since converting a raw
    /// line number into a scroll offset requires knowing the wrap width.
    pub fn jump_to_line(&mut self, line: u64) {
        self.pending_line = Some(line);
    }

    fn reset_row_cache(&mut self) {
        self.row_cache.clear();
        self.row_cache.push(0);
        self.row_cache_complete = false;
    }

    /// Rebuilds `row_cache` from scratch if `width` differs from what it was
    /// last built at. Cheap to call unconditionally; it's a no-op once the
    /// width matches.
    fn sync_row_cache_width(&mut self, width: u16) {
        if self.row_cache_width != width {
            self.row_cache_width = width;
            self.reset_row_cache();
        }
    }

    /// Extends `row_cache`, resuming from wherever it left off, until it has
    /// an entry for raw line `line_idx` or the file runs out. Used by
    /// `resolve_pending_jump`, which needs the row offset of one specific
    /// line and is only called on an actual jump, not every frame.
    fn ensure_row_cache_to_line(&mut self, buf: &mut TextBuffer, width: u16, line_idx: usize) {
        self.sync_text(buf);
        self.sync_row_cache_width(width);
        if self.row_cache_complete || self.row_cache.len() - 1 > line_idx {
            return;
        }
        let w = width.max(1) as usize;
        let already = self.row_cache.len() - 1;
        let mut rows = *self.row_cache.last().unwrap();
        let mut idx = already;
        while buf.ensure_line(idx) {
            rows += wrapped_row_count(&super::expand_tabs(&buf.line(idx).unwrap()), w) as u64;
            self.row_cache.push(rows);
            if self.row_cache.len() - 1 > line_idx {
                return;
            }
            idx += 1;
        }
        self.row_cache_complete = true;
    }

    /// Extends `row_cache` until its last entry exceeds `scroll` (i.e. it
    /// covers enough lines to locate row `scroll`) or the file runs out.
    /// This is the one `render` calls every frame — but since it only ever
    /// walks lines past what's already cached, a stationary or slowly
    /// moving scroll position costs nothing.
    fn ensure_row_cache_to_scroll(&mut self, buf: &mut TextBuffer, width: u16, scroll: u64) {
        self.sync_text(buf);
        self.sync_row_cache_width(width);
        if self.row_cache_complete {
            return;
        }
        let w = width.max(1) as usize;
        let already = self.row_cache.len() - 1;
        let mut rows = *self.row_cache.last().unwrap();
        if rows > scroll {
            return; // already covers it
        }
        let mut idx = already;
        while buf.ensure_line(idx) {
            rows += wrapped_row_count(&super::expand_tabs(&buf.line(idx).unwrap()), w) as u64;
            self.row_cache.push(rows);
            if rows > scroll {
                return;
            }
            idx += 1;
        }
        self.row_cache_complete = true;
    }

    /// Extends `row_cache` over every remaining line, so `row_cache.last()`
    /// becomes the true total wrapped-row count. Only called when that's
    /// actually needed (e.g. `End`, or a scroll target past what's been
    /// scanned so far) — not on every frame.
    fn ensure_row_cache_complete(&mut self, buf: &mut TextBuffer, width: u16) {
        self.sync_text(buf);
        self.sync_row_cache_width(width);
        if self.row_cache_complete {
            return;
        }
        let w = width.max(1) as usize;
        let already = self.row_cache.len() - 1;
        let mut rows = *self.row_cache.last().unwrap();
        let mut idx = already;
        while buf.ensure_line(idx) {
            rows += wrapped_row_count(&super::expand_tabs(&buf.line(idx).unwrap()), w) as u64;
            self.row_cache.push(rows);
            idx += 1;
        }
        self.row_cache_complete = true;
    }

    pub fn resolve_pending_jump(&mut self, buf: &mut TextBuffer, width: u16, highlight: Option<(&str, bool, bool, bool)>) {
        let Some(line) = self.pending_line.take() else { return };
        let line_idx = (line - 1) as usize;
        self.ensure_row_cache_to_line(buf, width, line_idx);
        if line_idx >= self.row_cache.len() - 1 {
            return; // text no longer has that many lines
        }
        let mut rows_before = self.row_cache[line_idx];
        if let Some(raw_line) = buf.line(line_idx) {
            let line_text = super::expand_tabs(&raw_line);
            let matcher = build_matcher(highlight);
            if let Some((m_start, _)) =
                matcher.as_ref().and_then(|m| m.find_matches(&line_text).into_iter().next())
            {
                rows_before += (wrapped_row_count(&line_text[..m_start], width as usize) - 1) as u64;
            }
        }
        self.scroll = rows_before.saturating_sub(2);
        self.pending_cap_scroll = true;
    }

    pub fn cap_scroll(&mut self, buf: &mut TextBuffer, width: u16, viewport_height: u16) {
        if !self.pending_cap_scroll {
            return;
        }
        self.pending_cap_scroll = false;

        // Only read as far as the view needs: if the text goes on past the
        // rows the view would show, the scroll position is in range. Reaching
        // the end of the text means the total row count is known to clamp to.
        let wanted = self.scroll.saturating_add(viewport_height as u64);
        self.ensure_row_cache_to_scroll(buf, width, wanted);
        if self.row_cache_complete {
            let total_rows = *self.row_cache.last().unwrap();
            let scroll_limit = total_rows.saturating_sub(viewport_height as u64);
            if self.scroll > scroll_limit {
                self.scroll = scroll_limit;
            }
        }
    }

        /// The view was moved by the user: the last search position no longer applies.
    fn moved(&mut self) {
        self.match_line = None;
        self.clear_feedback();
    }

    /// Drops the bottom-border message and disarms the wrap-around.
    pub fn clear_feedback(&mut self) {
        self.message = None;
        self.wrap_armed = None;
    }

    pub fn handle_key(&mut self, event: &KeyEvent, dismiss_bindings: &ActionBindings) -> OverlayOutcome {
        self.clear_feedback();
        if event.code == KeyCode::Esc || bindings_match_event(dismiss_bindings, event) {
            return OverlayOutcome::Dismissed;
        }
        match event.code {
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => {
                self.moved();
                self.scroll_key(event.code)
            }
            _ => OverlayOutcome::Passthrough,
        }
    }

    fn scroll_key(&mut self, code: KeyCode) -> OverlayOutcome {
        match code {
            KeyCode::Up => { self.scroll = self.scroll.saturating_sub(1); OverlayOutcome::Consumed }
            KeyCode::Down => { self.scroll = self.scroll.saturating_add(1); self.pending_cap_scroll = true; OverlayOutcome::Consumed }
            KeyCode::PageUp => { self.scroll = self.scroll.saturating_sub(20); OverlayOutcome::Consumed }
            KeyCode::PageDown => { self.scroll = self.scroll.saturating_add(20); self.pending_cap_scroll = true; OverlayOutcome::Consumed }
            KeyCode::Home => { self.scroll = 0; OverlayOutcome::Consumed }
            KeyCode::End => { self.scroll = u64::MAX; self.pending_cap_scroll = true; OverlayOutcome::Consumed }
            _ => OverlayOutcome::Passthrough,
        }
    }

    pub fn scroll_by(&mut self, delta: i16) {
        self.moved();
        if delta < 0 {
            self.scroll = self.scroll.saturating_sub((-delta) as u64);
        } else {
            self.scroll = self.scroll.saturating_add(delta as u64);
            self.pending_cap_scroll = true;
        }
    }

    pub fn scrollbar_click(&mut self, track_row: usize, inner_h: usize, total_lines: usize) {
        self.moved();
        if let Some(pos) = (track_row * total_lines).checked_div(inner_h) {
            if pos as u64 > self.scroll {
                self.pending_cap_scroll = true;
            }
            self.scroll = pos as u64;
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
/// Narrowest text column the viewer will give up the line-number gutter for.
const MIN_TEXT_WIDTH: u16 = 10;

pub fn content_width(area: Rect) -> u16 {
    area.width.saturating_sub(3)
}

/// Splits `line` into maximal runs of whitespace / non-whitespace characters,
/// in order, keeping every character (unlike `split_whitespace`, which drops
/// whitespace runs entirely) — needed because ratatui's `Wrap{trim:false}`
/// lays out whitespace verbatim rather than collapsing or trimming it. Each
/// segment is paired with whether it's a whitespace run.
fn whitespace_runs(line: &str) -> impl Iterator<Item = (bool, &str)> {
    let mut rest = line;
    std::iter::from_fn(move || {
        let is_ws = rest.chars().next()?.is_whitespace();
        let end = rest.find(|c: char| c.is_whitespace() != is_ws).unwrap_or(rest.len());
        let (segment, tail) = rest.split_at(end);
        rest = tail;
        Some((is_ws, segment))
    })
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
    pub text: &'a mut TextBuffer,
    pub title: &'a str,
    /// `(needle, case_sensitive, is_regex, whole_words)` — occurrences get the
    /// search-match colors.
    pub highlight: Option<(&'a str, bool, bool, bool)>,
    /// Show the source line number next to the first row of each line.
    pub line_numbers: bool,
}

impl<'a> StatefulWidget for OutputOverlayWidget<'a> {
    type State = OutputOverlayState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        Widget::render(Clear, area, buf);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(to_color(self.cs.dialog_border_fg)))
            .title(Span::from(super::sanitize_display(self.title)))
            .style(Style::default().bg(to_color(self.cs.panel_bg)));
        if let Some(message) = &state.message {
            block = block.title_bottom(Span::styled(
                format!(" {message} "),
                Style::default().fg(to_color(self.cs.status_warn_fg)).bg(to_color(self.cs.status_warn_bg)),
            ));
        }

        let inner = block.inner(area);
        block.render(area, buf);

        let style = Style::default()
            .fg(to_color(self.cs.panel_fg))
            .bg(to_color(self.cs.panel_bg));

        let text: &mut TextBuffer = self.text;
        state.sync_text(text);
        // Gutter: the line number plus one separating column, dropped when the
        // terminal is too narrow to leave room for the text.
        let gutter = if self.line_numbers {
            let digits = (text.estimated_lines().max(1).ilog10() + 1) as u16;
            state.gutter_digits = state.gutter_digits.max(digits);
            state.gutter_digits + 1
        } else {
            0
        };
        let full_width = content_width(area);
        let gutter = if full_width >= gutter + MIN_TEXT_WIDTH { gutter } else { 0 };
        let width = full_width - gutter;
        let text_area = Rect { x: inner.x + gutter, width, ..inner };
        // Resolve a pending jump first so the clamp below applies in the same frame;
        // otherwise a jump near the end of the text shows one overscrolled frame.
        state.resolve_pending_jump(text, width, self.highlight);
        state.cap_scroll(text, width, inner.height);

        // Cover the whole viewport so every visible line's first row is known.
        state.ensure_row_cache_to_scroll(text, width, state.scroll.saturating_add(inner.height as u64));
        let start_idx = state.row_cache.partition_point(|&r| r <= state.scroll) - 1;
        let rows_before = state.row_cache[start_idx];
        state.top_line = start_idx;
        let take_n = inner.height as usize + 1; // slack for a row scrolled mid-way into

        let total_lines = if state.row_cache_complete {
            *state.row_cache.last().unwrap()
        } else {
            *(state.row_cache.last().unwrap()).max(&(text.estimated_lines() as u64))
        };

        let matcher = build_matcher(self.highlight);
        let match_style = Style::default()
            .fg(to_color(self.cs.search_match_fg))
            .bg(to_color(self.cs.search_match_bg));
        let current_style = Style::default()
            .fg(to_color(self.cs.search_current_fg))
            .bg(to_color(self.cs.search_current_bg));
        let current_line = state.current_line;

        text.ensure_line(start_idx + take_n - 1);
        let text: &TextBuffer = text;
        let end_idx = (start_idx + take_n).min(text.known_lines());
        let lines: Vec<Line> = (start_idx..end_idx)
            .filter_map(|idx| text.line(idx).map(|line| (idx, line)))
            .map(|(idx, raw_line)| {
                let line = super::expand_tabs(&raw_line).into_owned();
                let Some(m) = matcher.as_ref() else {
                    return Line::from(Span::styled(line, style));
                };
                let mut spans = Vec::new();
                let mut pos = 0;
                for (n, (start, end)) in m.find_matches(&line).into_iter().enumerate() {
                    if start > pos {
                        spans.push(Span::styled(line[pos..start].to_string(), style));
                    }
                    // The first match of the current line is the one jumped to.
                    let this_style = if n == 0 && current_line == Some(idx) { current_style } else { match_style };
                    spans.push(Span::styled(line[start..end].to_string(), this_style));
                    pos = end;
                }
                if pos < line.len() {
                    spans.push(Span::styled(line[pos..].to_string(), style));
                }
                Line::from(spans)
            })
            .collect();

        let para = Paragraph::new(lines)
            .style(style)
            .wrap(Wrap { trim: false })
            .scroll(((state.scroll - rows_before).min(u16::MAX as u64) as u16, 0));
        Widget::render(para, text_area, buf);

        if gutter > 0 {
            let number_style = Style::default()
                .fg(to_color(self.cs.line_number_fg))
                .bg(to_color(self.cs.panel_bg));
            let digits = (gutter - 1) as usize;
            let bottom = state.scroll.saturating_add(inner.height as u64);
            for idx in start_idx..state.row_cache.len().min(text.known_lines()) {
                let first_row = state.row_cache[idx];
                if first_row >= bottom {
                    break;
                }
                if first_row < state.scroll {
                    continue; // this line's first row is scrolled off the top
                }
                let y = inner.y + (first_row - state.scroll) as u16;
                buf.set_stringn(inner.x, y, format!("{:>digits$}", idx + 1), digits, number_style);
            }
        }

        let scrollbar_area = Rect {
            x: inner.x + inner.width.saturating_sub(1),
            width: 1,
            ..inner
        };
        let mut scrollbar_state = ScrollbarState::new((1 + total_lines  as usize).saturating_sub(inner.height as usize))
            .position(state.scroll as usize);
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

    fn render_to_buffer(text: &str, area: Rect, state: &mut OutputOverlayState) -> Buffer {
        let cs = ColorScheme::default();
        let mut text = TextBuffer::from_string(text.to_string());
        let widget = OutputOverlayWidget { cs: &cs, text: &mut text, title: "test", highlight: None, line_numbers: false };
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(widget, area, &mut buf, state);
        buf
    }

    fn render_to_buffer_with_default_state(text: &str, area: Rect) -> Buffer {
        render_to_buffer(text, area, &mut OutputOverlayState::new())
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
        let buf = render_to_buffer_with_default_state(&text, area);
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
        let buf = render_to_buffer_with_default_state(&text, area);
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
        let buf = render_to_buffer_with_default_state(&text, area);
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
        state.resolve_pending_jump(&mut TextBuffer::from_string(text.clone()), content_width(area), None);

        let buf = render_to_buffer(&text, area, &mut state);
        assert!(
            find_row_containing(&buf, area, "needle-here").is_some(),
            "target line must be visible after jump_to_line despite earlier wrapping"
        );
    }

    /// A match deep inside a single very long wrapped line (e.g. a build `.d`
    /// file with one giant space-separated line) must itself end up visible —
    /// jumping to the start of that raw line isn't enough when the match is
    /// dozens of wrapped rows into it.
	fn jump_to_line_finds_a_match_deep_inside_a_long_wrapped_line() {
	    let words: Vec<String> = (0..40).map(|i| format!("filler-word-{i:03}-padded")).collect();
	    let mut long_line = words.join(" ");
	    long_line.push_str(" needle-token more-filler-after-the-match-token-here");
	    let text = format!("intro\n{long_line}\n");
	    let area = Rect::new(0, 0, 20, 8);

	    let mut state = OutputOverlayState::new();
	    state.jump_to_line(2);
	    state.resolve_pending_jump(&mut TextBuffer::from_string(text.clone()), content_width(area), Some(("needle-token", true, false, false)));

	    let buf = render_to_buffer(&text, area, &mut state);
	    assert!(
		find_row_containing(&buf, area, "needle-token").is_some(),
		"the match itself must be visible, not just the start of its (long) raw line"
	    );
	}

    #[test]
    fn resolve_pending_jump_handles_lines_beyond_u16_rows() {
        let text: String = (1..=70_000).map(|i| format!("line-{i}\n")).collect();
        let area = Rect::new(0, 0, 40, 10);
        let mut state = OutputOverlayState::new();
        state.jump_to_line(69_000);
        let buf = render_to_buffer(&text, area, &mut state);
        assert!(find_row_containing(&buf, area, "line-69000").is_some());
        state.handle_key(&KeyEvent::from(KeyCode::End), &ActionBindings::default());
        let buf = render_to_buffer(&text, area, &mut state);
        assert!(find_row_containing(&buf, area, "line-70000").is_some());
    }

    #[test]
    fn resolve_pending_jump_is_a_noop_without_a_pending_line() {
        let mut state = OutputOverlayState::new();
        state.scroll = 5;
        state.resolve_pending_jump(&mut TextBuffer::from_string("a\nb\nc\n".into()), 40, None);
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
        state.resolve_pending_jump(&mut TextBuffer::from_string("a\nb\nc\n".into()), 40, None);
        assert_eq!(state.scroll, 5);
    }

    /// A file of `n` numbered lines, read 4 KiB at a time.
    fn chunked_file(name: &str, n: usize) -> (std::path::PathBuf, TextBuffer) {
        let path = std::env::temp_dir().join(format!("sc-overlay-{}-{name}", std::process::id()));
        std::fs::write(&path, (1..=n).map(|i| format!("line-{i}\n")).collect::<String>()).unwrap();
        let buf = TextBuffer::open_with_chunk(path.to_str().unwrap(), 4096).unwrap();
        (path, buf)
    }

    fn render_buf(text: &mut TextBuffer, area: Rect, state: &mut OutputOverlayState) -> Buffer {
        let cs = ColorScheme::default();
        let widget = OutputOverlayWidget { cs: &cs, text, title: "test", highlight: None, line_numbers: false };
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(widget, area, &mut buf, state);
        buf
    }

    #[test]
    fn scrolling_down_reads_the_file_lazily() {
        let (path, mut text) = chunked_file("lazy", 20_000);
        let area = Rect::new(0, 0, 40, 10);
        let mut state = OutputOverlayState::new();
        render_buf(&mut text, area, &mut state);
        for _ in 0..30 {
            state.handle_key(&KeyEvent::from(KeyCode::PageDown), &ActionBindings::default());
            render_buf(&mut text, area, &mut state);
        }
        assert!(!text.fully_read(), "paging a little must not read the whole file");
        let buf = render_buf(&mut text, area, &mut state);
        assert!(find_row_containing(&buf, area, "line-601").is_some());
        state.handle_key(&KeyEvent::from(KeyCode::End), &ActionBindings::default());
        let buf = render_buf(&mut text, area, &mut state);
        assert!(text.fully_read());
        assert!(find_row_containing(&buf, area, "line-20000").is_some());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn jump_to_a_line_deep_in_a_multi_chunk_file() {
        let (path, mut text) = chunked_file("jump", 20_000);
        let area = Rect::new(0, 0, 40, 10);
        let mut state = OutputOverlayState::new();
        state.jump_to_line(15_000);
        let buf = render_buf(&mut text, area, &mut state);
        assert!(find_row_containing(&buf, area, "line-15000").is_some());
        assert!(!text.fully_read());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn showing_a_different_text_drops_the_row_cache() {
        let area = Rect::new(0, 0, 40, 10);
        let mut state = OutputOverlayState::new();
        let long: String = (1..=500).map(|i| format!("a-{i}\n")).collect();
        render_to_buffer(&long, area, &mut state);
        state.scroll = 300;
        render_to_buffer(&long, area, &mut state);
        assert!(state.row_cache.len() > 300);
        state.scroll = 0;
        let buf = render_to_buffer("b-1\nb-2\n", area, &mut state);
        assert!(state.row_cache.len() <= 3);
        assert!(find_row_containing(&buf, area, "b-1").is_some());
    }

    fn render_numbered(text: &str, area: Rect, state: &mut OutputOverlayState) -> Buffer {
        let cs = ColorScheme::default();
        let mut text = TextBuffer::from_string(text.to_string());
        let widget = OutputOverlayWidget { cs: &cs, text: &mut text, title: "test", highlight: None, line_numbers: true };
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(widget, area, &mut buf, state);
        buf
    }

    #[test]
    fn line_numbers_label_only_the_first_row_of_a_wrapped_line() {
        let area = Rect::new(0, 0, 24, 8);
        let text = format!("short\n{}\nlast\n", "w".repeat(60));
        let buf = render_numbered(&text, area, &mut OutputOverlayState::new());
        // 1 digit + 1 separator; text column starts at x = 1 (border) + 2.
        let rows: Vec<String> = (1..=6).map(|y| row_text(&buf, y, area)).collect();
        assert!(rows[0].starts_with("│1 short"), "{rows:?}");
        assert!(rows[1].starts_with("│2 www"), "{rows:?}");
        assert!(rows[2].starts_with("│  www"), "continuation rows get no number: {rows:?}");
        let last = rows.iter().find(|r| r.contains("last")).unwrap();
        assert!(last.starts_with("│3 last"), "{rows:?}");
    }

    #[test]
    fn line_numbers_follow_scrolling_and_hide_a_partly_scrolled_first_line() {
        let area = Rect::new(0, 0, 24, 6);
        let text = format!("{}\nsecond\nthird\n", "w".repeat(60));
        let mut state = OutputOverlayState::new();
        state.scroll = 1; // inside the wrapped first line
        let buf = render_numbered(&text, area, &mut state);
        let rows: Vec<String> = (1..=4).map(|y| row_text(&buf, y, area)).collect();
        assert!(rows[0].starts_with("│  www"), "no number on a continuation row: {rows:?}");
        assert!(rows.iter().any(|r| r.starts_with("│2 second")), "{rows:?}");
        assert!(!rows.iter().any(|r| r.starts_with("│1")), "{rows:?}");
    }

    #[test]
    fn line_numbers_are_right_aligned_to_the_widest_number() {
        let text: String = (1..=120).map(|i| format!("l{i}\n")).collect();
        let area = Rect::new(0, 0, 30, 6);
        let buf = render_numbered(&text, area, &mut OutputOverlayState::new());
        assert!(row_text(&buf, 1, area).starts_with("│  1 l1"));
        assert!(row_text(&buf, 2, area).starts_with("│  2 l2"));
    }

    #[test]
    fn no_gutter_when_line_numbers_are_off_or_the_terminal_is_too_narrow() {
        let area = Rect::new(0, 0, 24, 6);
        let buf = render_to_buffer("abc\n", area, &mut OutputOverlayState::new());
        assert!(row_text(&buf, 1, area).starts_with("│abc"));
        let narrow = Rect::new(0, 0, 12, 6);
        let buf = render_numbered("abc\n", narrow, &mut OutputOverlayState::new());
        assert!(row_text(&buf, 1, narrow).starts_with("│abc"), "gutter dropped when text would be cramped");
    }

    #[test]
    fn message_is_shown_in_the_bottom_border_and_top_line_tracks_the_scroll() {
        let area = Rect::new(0, 0, 40, 6);
        let text: String = (1..=30).map(|i| format!("line-{i}\n")).collect();
        let mut state = OutputOverlayState::new();
        state.message = Some("No more matches".to_string());
        state.scroll = 7;
        let buf = render_to_buffer(&text, area, &mut state);
        assert!(row_text(&buf, area.height - 1, area).contains("No more matches"));
        assert_eq!(state.top_line, 7);
        state.message = None;
        let buf = render_to_buffer(&text, area, &mut state);
        assert!(!row_text(&buf, area.height - 1, area).contains("No more matches"));
    }

    #[test]
    fn a_jump_near_the_end_is_clamped_in_the_same_frame() {
        // Jumping to the last line of a long text scrolls "2 rows above it", which is past
        // the maximum scroll. The first frame after the jump must already be clamped,
        // otherwise one overscrolled frame flickers on screen.
        let area = Rect::new(0, 0, 40, 6); // 4 text rows
        let text: String = (1..=30).map(|i| format!("line-{i}\n")).collect();
        let mut state = OutputOverlayState::new();
        state.jump_to_line(30);
        let buf = render_to_buffer(&text, area, &mut state);
        assert_eq!(state.scroll, 26);
        assert!(find_row_containing(&buf, area, "line-30").is_some());
        assert!(find_row_containing(&buf, area, "line-27").is_some());
    }

    #[test]
    fn the_current_match_has_its_own_colors() {
        let area = Rect::new(0, 0, 40, 6);
        let cs = ColorScheme::default();
        let mut text = TextBuffer::from_string("alpha\nfoo foo\nfoo\n".to_string());
        let mut state = OutputOverlayState::new();
        state.current_line = Some(1);
        let widget = OutputOverlayWidget {
            cs: &cs, text: &mut text, title: "t", highlight: Some(("foo", true, false, false)), line_numbers: false,
        };
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(widget, area, &mut buf, &mut state);
        let bg = |x: u16, y: u16| buf.cell((x, y)).unwrap().bg;
        let (cur, other) = (to_color(cs.search_current_bg), to_color(cs.search_match_bg));
        assert_eq!(bg(1, 2), cur, "first match of the current line");
        assert_eq!(bg(5, 2), other, "second match on the same line is an ordinary match");
        assert_eq!(bg(1, 3), other, "matches on other lines are ordinary");
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
        let buf = render_to_buffer_with_default_state(text, area);

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
