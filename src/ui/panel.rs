use crate::config::ColorScheme;
use crate::pattern::ContentMatcher;
use crate::provider::{LineMatch, NodeEntry, NodeKind, NodePath, SearchQuery, TreeProvider};
use crate::ui::modal_event::PanelOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use chrono::{DateTime, Local};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::cmp::Ordering;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Name,
    Extension,
    Size,
    Modified,
    Unsorted,
}

pub use crate::pattern::{build_filter_pattern, FilterPattern};

fn sort_entries(entries: &mut [NodeEntry], key: SortKey, asc: bool) {
    entries.sort_by(|a, b| {
        let dk = match (&a.kind, &b.kind) {
            (NodeKind::Dir, NodeKind::File) => Ordering::Less,
            (NodeKind::File, NodeKind::Dir) => Ordering::Greater,
            _ => Ordering::Equal,
        };
        if dk != Ordering::Equal { return dk; }
        let c = match key {
            SortKey::Unsorted => Ordering::Equal,
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Extension => {
                let ext_a = a.name.rsplit_once('.').map(|(_, e)| e).unwrap_or("").to_lowercase();
                let ext_b = b.name.rsplit_once('.').map(|(_, e)| e).unwrap_or("").to_lowercase();
                ext_a.cmp(&ext_b).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Modified => a.modified.cmp(&b.modified),
        };
        if asc { c } else { c.reverse() }
    });
}

/// State of a panel showing streaming search results in place of a directory.
/// The panel's `path` stays the search root; `entries` hold one row per hit,
/// named by their root-relative path.
pub struct SearchResultsState {
    pub query: SearchQuery,
    pub running: bool,
    /// Display path of the directory currently being scanned.
    pub scanning: Option<String>,
    /// Root-relative hit path → its matching lines (content searches).
    pub matches: HashMap<String, Vec<LineMatch>>,
    /// Set only when a `Done` event was actually received — i.e. the search
    /// ran to completion, as opposed to being interrupted by navigating away
    /// mid-scan. Drives the `(partial, Alt-r to refresh)` footer marker.
    pub complete: bool,
}

impl SearchResultsState {
    pub fn new(query: SearchQuery) -> Self {
        SearchResultsState { query, running: true, scanning: None, matches: HashMap::new(), complete: false }
    }

    pub fn content_search(&self) -> bool {
        self.query.content.is_some()
    }
}

/// State of a panel showing the matching lines of the file selected in the
/// results panel (content searches only). Navigation reuses the panel's
/// cursor/scroll; `entries` stays empty.
pub struct MatchesState {
    /// Root-relative path of the file whose matches are shown (sync key).
    pub file: Option<String>,
    /// Absolute path of that file (for the text viewer).
    pub abs_path: Option<String>,
    pub matches: Vec<LineMatch>,
    pub needle: String,
    pub case_sensitive: bool,
    pub content_is_regex: bool,
    pub content_whole_words: bool,
}

impl MatchesState {
    pub fn new(needle: String, case_sensitive: bool, content_is_regex: bool, content_whole_words: bool) -> Self {
        MatchesState {
            file: None,
            abs_path: None,
            matches: Vec::new(),
            needle,
            case_sensitive,
            content_is_regex,
            content_whole_words,
        }
    }
}

pub enum PanelContent {
    Dir,
    SearchResults(SearchResultsState),
    Matches(MatchesState),
}

pub struct PanelState {
    pub provider: Box<dyn TreeProvider>,
    pub path: NodePath,
    pub entries: Vec<NodeEntry>,
    pub cursor: usize,
    pub scroll: usize,
    pub tagged: HashSet<String>,
    pub error: Option<String>,
    pub sort_key: SortKey,
    pub sort_asc: bool,
    pub show_hidden: bool,
    pub filter: Option<FilterPattern>,
    pub content: PanelContent,
}

impl PanelState {
    pub fn new(provider: Box<dyn TreeProvider>, path: NodePath) -> Self {
        let mut s = PanelState {
            provider,
            path,
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            tagged: HashSet::new(),
            error: None,
            sort_key: SortKey::Name,
            sort_asc: true,
            show_hidden: false,
            filter: None,
            content: PanelContent::Dir,
        };
        s.refresh();
        s
    }

    /// Number of navigable rows: entries, or matching lines for a matches panel.
    pub fn item_count(&self) -> usize {
        match &self.content {
            PanelContent::Matches(m) => m.matches.len(),
            _ => self.entries.len(),
        }
    }

    pub fn refresh(&mut self) {
        match self.content {
            PanelContent::Dir => self.refresh_dir(),
            // Search hits don't come from `list`: just re-apply the sort order.
            PanelContent::SearchResults(_) => {
                sort_entries(&mut self.entries, self.sort_key, self.sort_asc);
                self.clamp_cursor();
            }
            PanelContent::Matches(_) => {}
        }
    }

    fn refresh_dir(&mut self) {
        match self.provider.list(&self.path) {
            Ok(mut entries) => {
                // Sort invariant is provided by the TreeProvider (dir-first, case-insensitive name).
                if self.provider.parent(&self.path).is_some() {
                    entries.insert(
                        0,
                        NodeEntry {
                            name: "..".to_string(),
                            kind: NodeKind::Dir,
                            size: 0,
                            modified: SystemTime::UNIX_EPOCH,
                            permissions: String::new(),
                        },
                    );
                }
                // Filter hidden files (dotfiles), keeping ".."
                if !self.show_hidden {
                    entries.retain(|e| e.name == ".." || !e.name.starts_with('.'));
                    // Untag any entries that are now hidden
                    self.tagged.retain(|n| !n.starts_with('.'));
                }
                // Apply filter pattern (always keep "..")
                if let Some(ref pat) = self.filter {
                    let hidden: HashSet<String> = entries.iter()
                        .filter(|e| {
                            e.name != ".."
                                && !(pat.files_only && e.kind == NodeKind::Dir)
                                && !pat.matches(&e.name)
                        })
                        .map(|e| e.name.clone())
                        .collect();
                    self.tagged.retain(|n| !hidden.contains(n));
                    entries.retain(|e| {
                        e.name == ".."
                            || (pat.files_only && e.kind == NodeKind::Dir)
                            || pat.matches(&e.name)
                    });
                }
                // Keep ".." at position 0, sort the rest
                let sort_start = if entries.first().map(|e| e.name == "..").unwrap_or(false) { 1 } else { 0 };
                sort_entries(&mut entries[sort_start..], self.sort_key, self.sort_asc);
                self.entries = entries;
                self.error = None;
            }
            Err(e) => {
                let is_not_found = e.root_cause()
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound);
                self.error = Some(if is_not_found {
                    format!("Cannot find directory: {}", self.path.0)
                } else {
                    e.to_string()
                });
                self.entries.clear();
                if self.provider.parent(&self.path).is_some() {
                    self.entries.push(NodeEntry {
                        name: "..".to_string(),
                        kind: NodeKind::Dir,
                        size: 0,
                        modified: SystemTime::UNIX_EPOCH,
                        permissions: String::new(),
                    });
                }
            }
        }
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        let count = self.item_count();
        if self.cursor >= count && count > 0 {
            self.cursor = count - 1;
        }
        if count == 0 {
            self.cursor = 0;
            self.scroll = 0;
        } else if self.scroll > self.cursor {
            self.scroll = self.cursor;
        }
    }

    pub fn current_entry(&self) -> Option<&NodeEntry> {
        self.entries.get(self.cursor)
    }

    pub fn current_name(&self) -> String {
        self.current_entry()
            .map(|e| e.name.clone())
            .unwrap_or_default()
    }

    pub fn enter_dir(&mut self) -> Option<String> {
        if !matches!(self.content, PanelContent::Dir) {
            return None; // hit activation is handled at the App level
        }
        let entry = self.entries.get(self.cursor)?;
        if entry.name == ".." {
            let parent = self.provider.parent(&self.path)?;
            self.path = parent;
            self.cursor = 0;
            self.scroll = 0;
            self.tagged.clear();
            self.refresh();
            return None;
        }
        if entry.kind != NodeKind::Dir {
            return None;
        }
        let new_path = self.provider.join(&self.path, &entry.name);
        if let Err(e) = self.provider.list(&new_path) {
            return Some(format!("Error: {}", e));
        }
        self.path = new_path;
        self.cursor = 0;
        self.scroll = 0;
        self.tagged.clear();
        self.refresh();
        None
    }

    pub fn move_cursor(&mut self, delta: i32, visible_height: usize) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        let new = (self.cursor as i32 + delta)
            .max(0)
            .min(count as i32 - 1) as usize;
        self.cursor = new;
        let vh = visible_height.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + vh {
            self.scroll = self.cursor + 1 - vh;
        }
    }

    pub fn handle_key(&mut self, event: &KeyEvent, visible_height: usize, action_mode: bool) -> PanelOutcome {
        if event.modifiers != KeyModifiers::NONE {
            return PanelOutcome::Passthrough;
        }
        match event.code {
            KeyCode::Up => { self.move_cursor(-1, visible_height); PanelOutcome::Consumed }
            KeyCode::Down => { self.move_cursor(1, visible_height); PanelOutcome::Consumed }
            KeyCode::PageUp => { self.move_cursor(-(visible_height as i32), visible_height); PanelOutcome::Consumed }
            KeyCode::PageDown => { self.move_cursor(visible_height as i32, visible_height); PanelOutcome::Consumed }
            KeyCode::Home if action_mode => { self.move_cursor(i32::MIN, visible_height); PanelOutcome::Consumed }
            KeyCode::End if action_mode => { self.move_cursor(i32::MAX / 2, visible_height); PanelOutcome::Consumed }
            KeyCode::Enter if action_mode => {
                // Non-Dir contents: Enter is intercepted at the App level.
                if matches!(self.content, PanelContent::Dir)
                    && self.current_entry().map(|e| e.kind == NodeKind::Dir).unwrap_or(false)
                {
                    if let Some(err) = self.enter_dir() {
                        return PanelOutcome::NavError(err);
                    }
                }
                PanelOutcome::Consumed
            }
            KeyCode::Enter => PanelOutcome::ExecuteCommand,
            _ => PanelOutcome::Passthrough,
        }
    }

    pub fn move_cursor_to_row(&mut self, row: usize, visible_height: usize) {
        let idx = self.scroll + row;
        if idx < self.item_count() {
            self.cursor = idx;
            let vh = visible_height.max(1);
            if self.cursor < self.scroll {
                self.scroll = self.cursor;
            } else if self.cursor >= self.scroll + vh {
                self.scroll = self.cursor + 1 - vh;
            }
        }
    }

    pub fn tag_toggle(&mut self, visible_height: usize, advance: bool) {
        if let Some(entry) = self.entries.get(self.cursor) {
            if entry.name != ".." {
                let name = entry.name.clone();
                if self.tagged.contains(&name) {
                    self.tagged.remove(&name);
                } else {
                    self.tagged.insert(name);
                }
            }
        }
        if advance {
            self.move_cursor(1, visible_height);
        }
    }

    pub fn invert_tags(&mut self) {
        let all: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.name != "..")
            .map(|e| e.name.clone())
            .collect();
        let new_tagged: HashSet<String> = all
            .into_iter()
            .filter(|n| !self.tagged.contains(n))
            .collect();
        self.tagged = new_tagged;
    }

    pub fn op_files(&self) -> Vec<String> {
        if !self.tagged.is_empty() {
            let mut v: Vec<String> = self.tagged.iter().cloned().collect();
            v.sort();
            v
        } else {
            self.current_entry()
                .filter(|e| e.name != "..")
                .map(|e| vec![e.name.clone()])
                .unwrap_or_default()
        }
    }
}

use super::to_color;

fn format_size(bytes: u64, kind: &NodeKind) -> String {
    if *kind == NodeKind::Dir {
        return "   <DIR>".to_string();
    }
    if bytes < 1024 {
        format!("{:>6} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:>5.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:>5.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:>5.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_date(t: SystemTime, fmt: &str, fallback_len: usize) -> String {
    let dt: DateTime<Local> = t.into();
    use std::fmt::Write as _;
    let mut buf = String::new();
    if write!(buf, "{}", dt.format(fmt)).is_err() {
        return "!".repeat(fallback_len);
    }
    buf
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        format!("{:<width$}", s, width = max)
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('~');
        out
    }
}

/// Byte offset of the `n`th char in `s` (or `s.len()` if `s` has fewer than `n` chars).
fn byte_of_char(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(b, _)| b).unwrap_or(s.len())
}

/// Truncates a matches-panel line to `max` chars for display. If the full line
/// already fits, it's returned unchanged (padded) and `hits` unchanged. Otherwise,
/// if `hits` has a first entry, the visible window is centered on it (truncating
/// the start and/or end as needed, each marked with a single `~`) so a match far
/// into the line stays visible instead of being silently cut off. With no hits,
/// falls back to front-only truncation (matches `truncate_str`'s behavior).
/// `hits` are byte ranges into `text`; returns them translated into byte ranges
/// valid for the returned text, dropping any hit that falls entirely outside the
/// visible window.
fn truncate_matches_line(text: &str, hits: &[(usize, usize)], max: usize) -> (String, Vec<(usize, usize)>) {
    let total_chars = text.chars().count();
    if total_chars <= max {
        return (format!("{:<width$}", text, width = max), hits.to_vec());
    }
    let Some(&(first_start, first_end)) = hits.first().filter(|_| max >= 4) else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('~');
        return (out, Vec::new());
    };

    let match_start_char = text[..first_start].chars().count();
    let match_end_char = text[..first_end].chars().count();
    let center_char = (match_start_char + match_end_char) / 2;

    // Pass 1: assume both markers are needed, to get a safe (upper-bound) read on
    // which side(s) actually get truncated.
    let window = |content_w: usize| -> (usize, usize) {
        let content_w = content_w.max(1);
        let mut start = center_char.saturating_sub(content_w / 2);
        if start + content_w > total_chars {
            start = total_chars.saturating_sub(content_w);
        }
        (start, (start + content_w).min(total_chars))
    };
    let (start1, end1) = window(max.saturating_sub(2));
    let need_lead = start1 > 0;
    let need_trail = end1 < total_chars;

    // Pass 2: reserve only the marker(s) actually needed, freeing up any column
    // pass 1 conservatively set aside for a marker that turns out unnecessary.
    let reserve = need_lead as usize + need_trail as usize;
    let (start, end) = window(max - reserve);
    let need_lead = start > 0;
    let need_trail = end < total_chars;

    let start_byte = byte_of_char(text, start);
    let end_byte = byte_of_char(text, end);
    let lead: &str = if need_lead { "~" } else { "" };
    let trail: &str = if need_trail { "~" } else { "" };
    let windowed = format!("{lead}{}{trail}", &text[start_byte..end_byte]);
    let padded = format!("{:<width$}", windowed, width = max);

    let translated = hits
        .iter()
        .filter_map(|&(h_start, h_end)| {
            let clip_start = h_start.max(start_byte);
            let clip_end = h_end.min(end_byte);
            if clip_start >= clip_end {
                return None;
            }
            let offset = lead.len() + clip_start - start_byte;
            Some((offset, offset + (clip_end - clip_start)))
        })
        .collect();

    (padded, translated)
}

/// Truncates from the front with a leading `...`, keeping the tail (the most specific,
/// currently-relevant part of a path) visible instead of the start.
fn truncate_path_front(s: &str, max: usize) -> String {
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    const ELLIPSIS: &str = "...";
    if max <= ELLIPSIS.chars().count() {
        return s.chars().skip(len - max).collect();
    }
    let keep = max - ELLIPSIS.chars().count();
    let tail: String = s.chars().skip(len - keep).collect();
    format!("{ELLIPSIS}{tail}")
}

pub struct PanelWidget<'a> {
    pub cs: &'a ColorScheme,
    pub active: bool,
    pub title: String,
    pub time_format: &'a str,
    pub time_length: usize,
}

/// ASCII spinner frame for the results-panel footer, driven by the event
/// loop's 50ms redraw tick.
fn spinner_frame() -> char {
    const FRAMES: [char; 4] = ['|', '/', '-', '\\'];
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    FRAMES[((ms / 120) % FRAMES.len() as u128) as usize]
}

impl<'a> StatefulWidget for PanelWidget<'a> {
    type State = PanelState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if matches!(state.content, PanelContent::Matches(_)) {
            self.render_matches(area, buf, state);
            return;
        }
        let border_color = if self.active {
            to_color(self.cs.active_border_fg)
        } else {
            to_color(self.cs.inactive_border_fg)
        };

        let tagged_count = state.tagged.len();
        let (footer_text, footer_style) = if let Some(err) = &state.error {
            (format!(" {} ", err), Style::default().fg(to_color(self.cs.panel_error_fg)))
        } else {
            let has_dotdot = state.entries.first().map(|e| e.name == "..").unwrap_or(false);
            let total = state.entries.len().saturating_sub(has_dotdot as usize);
            let mut text = if tagged_count > 0 {
                format!(" {}/{} tagged ", tagged_count, total)
            } else {
                format!(" {} entries ", total)
            };
            if let PanelContent::SearchResults(sr) = &state.content {
                if sr.running {
                    if let Some(scan) = &sr.scanning {
                        // The spinner is a right-aligned bottom title, and ratatui
                        // draws right titles before left ones — keep the footer
                        // short enough not to paint over it.
                        let footer_max = (area.width as usize).saturating_sub(2 + 3);
                        let path_max = footer_max
                            .saturating_sub(text.chars().count() + "Searching  ".len());
                        if path_max >= 4 {
                            text.push_str(&format!(
                                "Searching {} ",
                                truncate_path_front(scan, path_max)
                            ));
                        }
                    }
                } else if !sr.complete {
                    // No spinner competes for space here, but a narrow panel
                    // still needs to degrade gracefully rather than overflow.
                    const FULL: &str = "(partial, Alt-r to refresh) ";
                    const SHORT: &str = "(partial) ";
                    let footer_max = (area.width as usize).saturating_sub(4);
                    let avail = footer_max.saturating_sub(text.chars().count());
                    if avail >= FULL.chars().count() {
                        text.push_str(FULL);
                    } else if avail >= SHORT.chars().count() {
                        text.push_str(SHORT);
                    }
                }
            }
            (text, Style::default().fg(border_color))
        };

        // 2 corners + 2 padding spaces around the title text itself.
        let title_max = (area.width as usize).saturating_sub(4);
        let title_text = truncate_path_front(&self.title, title_max);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" {} ", title_text),
                Style::default().fg(border_color),
            ))
            .title_bottom(Span::styled(footer_text, footer_style));
        if matches!(&state.content, PanelContent::SearchResults(sr) if sr.running) {
            block = block.title_bottom(
                Line::from(Span::styled(
                    format!(" {} ", spinner_frame()),
                    Style::default().fg(border_color),
                ))
                .alignment(Alignment::Right),
            );
        }

        let inner = block.inner(area);
        block.render(area, buf);

        // First row of inner is the header; entries occupy the rest.
        let header_y = inner.y;
        let entries_height = inner.height.saturating_sub(1);
        let entries_y = inner.y + 1;
        let visible_height = entries_height as usize;

        // Ensure scroll is consistent with cursor
        if state.cursor < state.scroll {
            state.scroll = state.cursor;
        } else if visible_height > 0 && state.cursor >= state.scroll + visible_height {
            state.scroll = state.cursor + 1 - visible_height;
        }

        let available_width = inner.width as usize;
        let time_length = self.time_length;
        // Per-file match counts, shown as an extra column for content searches.
        let match_counts = match &state.content {
            PanelContent::SearchResults(sr) if sr.content_search() => Some(&sr.matches),
            _ => None,
        };
        let count_w = if match_counts.is_some() { 6 } else { 0 };
        // columns: tag(1) + space(1) + name + space(1) + [count(5) + space(1)] + size(8) + space(1) + date(time_length)
        let fixed = 1 + 1 + 1 + 8 + 1 + time_length + count_w;
        let name_width = if available_width > fixed + 4 {
            available_width - fixed
        } else {
            4
        };

        // Render column header row
        {
            let hdr_style = Style::default()
                .fg(border_color)
                .bg(to_color(self.cs.panel_bg))
                .add_modifier(Modifier::BOLD);
            let name_active = matches!(state.sort_key, SortKey::Name | SortKey::Extension);
            let size_active = state.sort_key == SortKey::Size;
            let date_active = state.sort_key == SortKey::Modified;
            let icon = |active: bool| -> &'static str {
                if !active { " " } else if state.sort_asc { "▲" } else { "▼" }
            };
            let name_label = if let PanelContent::SearchResults(sr) = &state.content {
                format!("Name{} [{}]", icon(name_active), sr.query.pattern)
            } else if let Some(ref pat) = state.filter {
                format!("Name{} [{}]", icon(name_active), pat.raw)
            } else {
                format!("Name{}", icon(name_active))
            };
            let name_hdr = truncate_str(&name_label, name_width);
            let count_hdr = if match_counts.is_some() { "Match " } else { "" };
            let size_label = format!("  Size{} ", icon(size_active));  // 8 chars
            let size_hdr = format!("{:<8}", size_label);
            let date_label = format!(" Mtime{}", icon(date_active));
            let date_hdr = truncate_str(&date_label, time_length);
            let hdr_text = format!("  {} {}{} {}", name_hdr, count_hdr, size_hdr, date_hdr);
            // Pad / truncate to inner width
            let padded = format!("{:<width$}", hdr_text, width = available_width);
            let display: String = padded.chars().take(available_width).collect();
            buf.set_string(inner.x, header_y, &display, hdr_style);
        }

        let visible: Vec<&NodeEntry> = state
            .entries
            .iter()
            .skip(state.scroll)
            .take(visible_height)
            .collect();

        let items: Vec<ListItem> = visible
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let abs_idx = state.scroll + i;
                let is_tagged = state.tagged.contains(&entry.name);
                let is_cursor = abs_idx == state.cursor;

                let tag_ch = if is_tagged { '*' } else { ' ' };
                let name_part = truncate_str(&entry.name, name_width);
                let count_part = match match_counts {
                    Some(counts) => format!(
                        "{:>5} ",
                        counts.get(&entry.name).map(|m| m.len()).unwrap_or(0)
                    ),
                    None => String::new(),
                };
                let size_part = if entry.name == ".." {
                    "        ".to_string()
                } else {
                    format_size(entry.size, &entry.kind)
                };
                let date_part = if entry.name == ".." {
                    " ".repeat(time_length)
                } else {
                    truncate_str(&format_date(entry.modified, self.time_format, time_length), time_length)
                };

                let base_style = if is_cursor && self.active {
                    Style::default()
                        .fg(to_color(self.cs.selected_fg))
                        .bg(to_color(self.cs.selected_bg))
                } else if is_tagged {
                    Style::default()
                        .fg(to_color(self.cs.tagged_fg))
                        .bg(to_color(self.cs.tagged_bg))
                } else if entry.kind == NodeKind::Dir {
                    Style::default()
                        .fg(to_color(self.cs.active_border_fg))
                        .bg(to_color(self.cs.panel_bg))
                } else {
                    Style::default()
                        .fg(to_color(self.cs.panel_fg))
                        .bg(to_color(self.cs.panel_bg))
                };

                let text = format!(
                    "{} {} {}{} {}",
                    tag_ch, name_part, count_part, size_part, date_part
                );
                ListItem::new(Line::from(Span::styled(text, base_style)))
            })
            .collect();

        let mut list_state = ListState::default();
        if state.cursor >= state.scroll {
            let rel = state.cursor - state.scroll;
            if rel < visible_height {
                list_state.select(Some(rel));
            }
        }

        let list = List::new(items)
            .style(Style::default().bg(to_color(self.cs.panel_bg)))
            .highlight_style(
                Style::default()
                    .fg(to_color(self.cs.selected_fg))
                    .bg(to_color(self.cs.selected_bg))
                    .add_modifier(Modifier::BOLD),
            );

        let entries_area = Rect { x: inner.x, y: entries_y, width: inner.width, height: entries_height };
        StatefulWidget::render(list, entries_area, buf, &mut list_state);
    }
}

impl<'a> PanelWidget<'a> {
    /// Two-column (line number, text) view of the matching lines of the file
    /// selected in the results panel, with the matched substring highlighted.
    fn render_matches(self, area: Rect, buf: &mut Buffer, state: &mut PanelState) {
        let border_color = if self.active {
            to_color(self.cs.active_border_fg)
        } else {
            to_color(self.cs.inactive_border_fg)
        };

        let visible_height = area.height.saturating_sub(3).max(1) as usize;
        if state.cursor < state.scroll {
            state.scroll = state.cursor;
        } else if state.cursor >= state.scroll + visible_height {
            state.scroll = state.cursor + 1 - visible_height;
        }
        let (cursor, scroll) = (state.cursor, state.scroll);

        let PanelContent::Matches(ref ms) = state.content else { return };

        let title_max = (area.width as usize).saturating_sub(4);
        let title_text = truncate_path_front(&self.title, title_max);
        let footer = format!(" {} matches ", ms.matches.len());

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(format!(" {} ", title_text), Style::default().fg(border_color)))
            .title_bottom(Span::styled(footer, Style::default().fg(border_color)));
        let inner = block.inner(area);
        block.render(area, buf);

        let num_w = ms
            .matches
            .last()
            .map(|m| m.line.to_string().len())
            .unwrap_or(1)
            .max(4);
        let text_w = (inner.width as usize).saturating_sub(num_w + 2);

        // Header row
        let hdr_style = Style::default()
            .fg(border_color)
            .bg(to_color(self.cs.panel_bg))
            .add_modifier(Modifier::BOLD);
        let hdr = format!(" {:>num_w$} Text", "Line");
        let padded = format!("{:<width$}", hdr, width = inner.width as usize);
        let display: String = padded.chars().take(inner.width as usize).collect();
        buf.set_string(inner.x, inner.y, &display, hdr_style);

        let base_style = Style::default()
            .fg(to_color(self.cs.panel_fg))
            .bg(to_color(self.cs.panel_bg));
        let num_style = Style::default()
            .fg(border_color)
            .bg(to_color(self.cs.panel_bg));
        let match_style = Style::default()
            .fg(to_color(self.cs.search_match_fg))
            .bg(to_color(self.cs.search_match_bg));
        let selected_style = Style::default()
            .fg(to_color(self.cs.selected_fg))
            .bg(to_color(self.cs.selected_bg));

        let matcher = ContentMatcher::build(
            &ms.needle,
            ms.content_is_regex,
            ms.case_sensitive,
            ms.content_whole_words,
        )
        .ok();

        let items: Vec<ListItem> = ms
            .matches
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_height)
            .map(|(idx, m)| {
                let is_cursor = idx == cursor && self.active;
                // Tabs must be expanded before truncation/highlighting: a raw
                // tab written to the terminal jumps the physical cursor past
                // what ratatui's buffer model expects, overwriting the
                // panel's own border on that row.
                let expanded = super::expand_tabs(&m.text);
                let full_hits = matcher.as_ref().map(|mm| mm.find_matches(&expanded)).unwrap_or_default();
                let (text, hits) = truncate_matches_line(&expanded, &full_hits, text_w);
                let num = format!(" {:>num_w$} ", m.line);
                let line = if is_cursor {
                    Line::from(Span::styled(format!("{}{}", num, text), selected_style))
                } else {
                    let mut spans = vec![Span::styled(num, num_style)];
                    let mut pos = 0;
                    for (start, end) in hits {
                        if start > pos {
                            spans.push(Span::styled(text[pos..start].to_string(), base_style));
                        }
                        spans.push(Span::styled(text[start..end].to_string(), match_style));
                        pos = end;
                    }
                    if pos < text.len() {
                        spans.push(Span::styled(text[pos..].to_string(), base_style));
                    }
                    Line::from(spans)
                };
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).style(Style::default().bg(to_color(self.cs.panel_bg)));
        let rows_area = Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };
        Widget::render(list, rows_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_path_front_fits_unchanged() {
        assert_eq!(truncate_path_front("/tmp/short", 20), "/tmp/short");
    }

    #[test]
    fn truncate_path_front_collapses_with_leading_ellipsis() {
        assert_eq!(truncate_path_front("/home/alice/projects/sc", 15), ".../projects/sc");
    }

    #[test]
    fn truncate_path_front_exact_fit_unchanged() {
        assert_eq!(truncate_path_front("/abcde", 6), "/abcde");
    }

    #[test]
    fn truncate_path_front_keeps_tail_when_too_narrow_for_ellipsis() {
        assert_eq!(truncate_path_front("/home/alice/projects", 2), "ts");
    }

    #[test]
    fn truncate_matches_line_fits_unchanged() {
        let (out, hits) = truncate_matches_line("hello cat world", &[(6, 9)], 20);
        assert_eq!(out, format!("{:<20}", "hello cat world"));
        assert_eq!(hits, vec![(6, 9)]);
    }

    #[test]
    fn truncate_matches_line_match_near_start_needs_no_leading_marker() {
        let text = "cat0123456789ABCDEFGHIJ";
        let (out, hits) = truncate_matches_line(text, &[(0, 3)], 10);
        assert_eq!(out, "cat012345~");
        assert_eq!(hits, vec![(0, 3)]);
        assert_eq!(&out[hits[0].0..hits[0].1], "cat");
    }

    #[test]
    fn truncate_matches_line_centers_match_with_both_markers() {
        let text = format!("{}cat{}", "A".repeat(20), "B".repeat(20));
        let (out, hits) = truncate_matches_line(&text, &[(20, 23)], 10);
        assert_eq!(out, "~AAAcatBB~");
        assert_eq!(hits, vec![(4, 7)]);
        assert_eq!(&out[hits[0].0..hits[0].1], "cat");
    }

    #[test]
    fn truncate_matches_line_match_near_end_needs_no_trailing_marker() {
        let text = format!("{}cat", "A".repeat(30));
        let (out, hits) = truncate_matches_line(&text, &[(30, 33)], 10);
        assert_eq!(out, "~AAAAAAcat");
        assert_eq!(hits, vec![(7, 10)]);
        assert_eq!(&out[hits[0].0..hits[0].1], "cat");
    }

    #[test]
    fn truncate_matches_line_no_hits_falls_back_to_trailing_marker_only() {
        let text = "A".repeat(20);
        let (out, hits) = truncate_matches_line(&text, &[], 10);
        assert_eq!(out, "AAAAAAAAA~");
        assert!(hits.is_empty());
    }

    #[test]
    fn truncate_matches_line_too_narrow_falls_back_regardless_of_hits() {
        let text = format!("{}cat{}", "A".repeat(20), "B".repeat(20));
        let (out, hits) = truncate_matches_line(&text, &[(20, 23)], 3);
        assert_eq!(out, "AA~");
        assert!(hits.is_empty());
    }

    #[test]
    fn format_date_invalid_format_string_falls_back_to_exclamation_fill() {
        let out = format_date(SystemTime::now(), "%Q", 14);
        assert_eq!(out, "!".repeat(14));
    }

    #[test]
    fn format_date_valid_format_string_still_works() {
        let out = format_date(SystemTime::UNIX_EPOCH, "%Y", 20);
        assert!(!out.contains('!'));
    }

    use crate::provider::filesystem::FilesystemProvider;

    fn make_dirs(name: &str, depth: usize) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("sc_panel_test_{name}"));
        let _ = std::fs::remove_dir_all(&base);
        let mut leaf = base.clone();
        for i in 0..depth {
            leaf = leaf.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&leaf).unwrap();
        base
    }

    #[test]
    fn enter_dir_parent_missing_advances_one_level_with_error_footer() {
        let base = make_dirs("parent_missing", 2); // base/d0/d1
        let leaf = NodePath(base.join("d0").join("d1").to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), leaf);

        std::fs::remove_dir_all(base.join("d0")).unwrap();

        let result = panel.enter_dir(); // cursor starts on ".."
        assert_eq!(result, None);
        assert_eq!(panel.path.0, base.join("d0").to_string_lossy());
        assert!(panel.error.as_deref().unwrap_or("").contains("Cannot find directory"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enter_dir_two_missing_levels_requires_two_calls() {
        let base = make_dirs("two_missing", 3); // base/d0/d1/d2
        let leaf = NodePath(base.join("d0").join("d1").join("d2").to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), leaf);

        std::fs::remove_dir_all(base.join("d0").join("d1")).unwrap();

        // First call lands on the still-missing intermediate parent: no skip.
        assert_eq!(panel.enter_dir(), None);
        assert_eq!(panel.path.0, base.join("d0").join("d1").to_string_lossy());
        assert!(panel.error.is_some());

        // Second call reaches the real, listable ancestor.
        assert_eq!(panel.enter_dir(), None);
        assert_eq!(panel.path.0, base.join("d0").to_string_lossy());
        assert!(panel.error.is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enter_dir_into_existing_subdirectory_still_works() {
        let base = make_dirs("subdir_ok", 1); // base/d0
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root);

        let idx = panel.entries.iter().position(|e| e.name == "d0").unwrap();
        panel.cursor = idx;
        assert_eq!(panel.enter_dir(), None);
        assert_eq!(panel.path.0, base.join("d0").to_string_lossy());
        assert!(panel.error.is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    fn entry(name: &str, kind: NodeKind, size: u64) -> NodeEntry {
        NodeEntry {
            name: name.to_string(),
            kind,
            size,
            modified: SystemTime::UNIX_EPOCH,
            permissions: String::new(),
        }
    }

    fn test_query() -> SearchQuery {
        SearchQuery {
            pattern: "*".to_string(),
            is_regex: false,
            case_sensitive: true,
            content: None,
            content_is_regex: false,
            content_case_sensitive: true,
            content_whole_words: false,
            max_depth: None,
            include_hidden: false,
            follow_symlinks: false,
        }
    }

    #[test]
    fn refresh_on_results_panel_keeps_hits_and_applies_sort() {
        let base = make_dirs("results_refresh", 0);
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root);
        panel.content = PanelContent::SearchResults(SearchResultsState::new(test_query()));
        panel.entries = vec![
            entry("z/b.rs", NodeKind::File, 10),
            entry("a/a.rs", NodeKind::File, 20),
        ];
        panel.refresh();
        assert_eq!(panel.entries.len(), 2, "hits must survive refresh");
        assert_eq!(panel.entries[0].name, "a/a.rs"); // Name asc applied

        panel.sort_key = SortKey::Size;
        panel.sort_asc = false;
        panel.refresh();
        assert_eq!(panel.entries[0].name, "a/a.rs"); // 20 bytes first, desc

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn matches_panel_cursor_moves_over_match_lines() {
        let base = make_dirs("matches_cursor", 0);
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root);
        let mut ms = MatchesState::new("x".to_string(), true, false, false);
        ms.matches = vec![
            LineMatch { line: 1, text: "x1".into() },
            LineMatch { line: 5, text: "x2".into() },
            LineMatch { line: 9, text: "x3".into() },
        ];
        panel.content = PanelContent::Matches(ms);
        panel.entries.clear();
        panel.cursor = 0;

        panel.move_cursor(1, 10);
        assert_eq!(panel.cursor, 1);
        panel.move_cursor(10, 10);
        assert_eq!(panel.cursor, 2, "clamped to the last match line");
        assert_eq!(panel.item_count(), 3);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn matches_panel_keeps_a_far_right_match_visible() {
        let base = make_dirs("matches_far_right", 0);
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root);
        let mut ms = MatchesState::new("cat".to_string(), true, false, false);
        let line = format!("{}cat{}", "A".repeat(30), "B".repeat(30));
        ms.matches = vec![LineMatch { line: 1, text: line }];
        panel.content = PanelContent::Matches(ms);
        panel.entries.clear();
        panel.cursor = 0;

        let cs = ColorScheme::default();
        let widget = PanelWidget {
            cs: &cs,
            active: false, // avoid cursor-row styling so the match highlight is exercised
            title: "matches".into(),
            time_format: "%y-%m-%d %H:%M",
            time_length: 14,
        };
        let area = Rect::new(0, 0, 30, 10);
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(widget, area, &mut buf, &mut panel);

        let row: String = (0..area.width)
            .map(|x| buf.cell((x, 2)).unwrap().symbol().chars().next().unwrap())
            .collect();
        assert!(row.contains("cat"), "match must stay visible in the truncated row: {row:?}");
        assert!(row.contains('~'), "long line must show a truncation marker: {row:?}");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A raw tab written straight to the terminal jumps the physical cursor
    /// past what ratatui expects, overwriting the panel's own border on that
    /// row — tabs must be expanded to spaces before rendering.
    #[test]
    fn matches_panel_expands_tabs_before_rendering() {
        let base = make_dirs("matches_tabs", 0);
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root);
        let mut ms = MatchesState::new("needle".to_string(), true, false, false);
        ms.matches = vec![LineMatch { line: 1, text: "before\ttab\tneedle".to_string() }];
        panel.content = PanelContent::Matches(ms);
        panel.entries.clear();
        panel.cursor = 0;

        let cs = ColorScheme::default();
        let widget = PanelWidget {
            cs: &cs,
            active: false,
            title: "matches".into(),
            time_format: "%y-%m-%d %H:%M",
            time_length: 14,
        };
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(widget, area, &mut buf, &mut panel);

        let row: String = (0..area.width)
            .map(|x| buf.cell((x, 2)).unwrap().symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(!row.contains('\t'), "raw tabs must be expanded before rendering: {row:?}");
        assert!(row.contains("needle"), "match text must still be present: {row:?}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enter_dir_is_a_noop_on_search_panels() {
        let base = make_dirs("results_enter", 1); // base/d0
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root.clone());
        panel.content = PanelContent::SearchResults(SearchResultsState::new(test_query()));
        panel.entries = vec![entry("d0", NodeKind::Dir, 0)];
        panel.cursor = 0;
        assert_eq!(panel.enter_dir(), None);
        assert_eq!(panel.path, root, "path must not change; App handles hits");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enter_dir_into_missing_subdirectory_blocks_navigation() {
        let base = make_dirs("subdir_missing", 1); // base/d0
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root);

        let idx = panel.entries.iter().position(|e| e.name == "d0").unwrap();
        panel.cursor = idx;
        std::fs::remove_dir_all(base.join("d0")).unwrap();

        let result = panel.enter_dir();
        assert!(result.unwrap().starts_with("Error:"));
        assert_eq!(panel.path.0, base.to_string_lossy());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn results_footer_never_paints_over_the_spinner() {
        let base = make_dirs("spinner_footer", 1);
        let root = NodePath(base.to_string_lossy().into_owned());
        let mut panel = PanelState::new(Box::new(FilesystemProvider), root);
        let mut sr = SearchResultsState::new(SearchQuery {
            pattern: "*".into(),
            is_regex: false,
            case_sensitive: false,
            content: None,
            content_is_regex: false,
            content_case_sensitive: false,
            content_whole_words: false,
            max_depth: None,
            include_hidden: false,
            follow_symlinks: false,
        });
        // Long enough to fill the whole footer if not truncated.
        sr.scanning = Some("/very/long/path/segment/after/segment/that/never/ends".into());
        panel.content = PanelContent::SearchResults(sr);

        let cs = ColorScheme::default();
        let widget = PanelWidget {
            cs: &cs,
            active: true,
            title: "results".into(),
            time_format: "%y-%m-%d %H:%M",
            time_length: 14,
        };
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(widget, area, &mut buf, &mut panel);

        let bottom: String = (0..area.width)
            .map(|x| buf.cell((x, area.height - 1)).unwrap().symbol().chars().next().unwrap())
            .collect();
        // Expect the row to end with ` <frame> ┘`.
        let tail: Vec<char> = bottom.chars().rev().take(4).collect();
        assert!(
            ['|', '/', '-', '\\'].contains(&tail[2]) && tail[1] == ' ' && tail[3] == ' ',
            "no spinner at footer tail of {bottom:?}"
        );
        assert!(bottom.contains("Searching"), "searching text missing: {bottom:?}");

        let _ = std::fs::remove_dir_all(&base);
    }
}
