use crate::config::ColorScheme;
use crate::provider::{NodeEntry, NodeKind, NodePath, TreeProvider};
use crate::ui::modal_event::PanelOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use chrono::{DateTime, Local};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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

/// A compiled filter/select-group pattern.
#[derive(Debug, Clone)]
pub struct FilterPattern {
    pub raw: String,
    pub files_only: bool,
    pub case_sensitive: bool,
    pub is_regex: bool,
    regex: Option<regex::Regex>,
    glob: Option<glob::Pattern>,
}

impl FilterPattern {
    /// Returns true if `name` matches the pattern, respecting case sensitivity.
    /// Does NOT apply the `files_only` flag — callers handle that themselves.
    pub fn matches(&self, name: &str) -> bool {
        if self.is_regex {
            self.regex.as_ref().is_some_and(|r| r.is_match(name))
        } else {
            let opts = glob::MatchOptions {
                case_sensitive: self.case_sensitive,
                require_literal_separator: false,
                require_literal_leading_dot: false,
            };
            self.glob.as_ref().is_some_and(|g| g.matches_with(name, opts))
        }
    }
}

/// Build and compile a filter/select pattern with explicit options.
/// Returns `Err(description)` on invalid patterns.
pub fn build_filter_pattern(
    text: &str,
    files_only: bool,
    case_sensitive: bool,
    is_regexp: bool,
) -> Result<FilterPattern, String> {
    if is_regexp {
        let mut builder = regex::RegexBuilder::new(text);
        builder.case_insensitive(!case_sensitive);
        match builder.build() {
            Ok(r) => Ok(FilterPattern {
                raw: text.to_string(),
                files_only,
                case_sensitive,
                is_regex: true,
                regex: Some(r),
                glob: None,
            }),
            Err(e) => Err(format!("Invalid regex: {e}")),
        }
    } else {
        match glob::Pattern::new(text) {
            Ok(g) => Ok(FilterPattern {
                raw: text.to_string(),
                files_only,
                case_sensitive,
                is_regex: false,
                regex: None,
                glob: Some(g),
            }),
            Err(e) => Err(format!("Invalid glob: {e}")),
        }
    }
}

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
        };
        s.refresh();
        s
    }

    pub fn refresh(&mut self) {
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
        if self.cursor >= self.entries.len() && !self.entries.is_empty() {
            self.cursor = self.entries.len() - 1;
        }
        if self.entries.is_empty() {
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
        if self.entries.is_empty() {
            return;
        }
        let new = (self.cursor as i32 + delta)
            .max(0)
            .min(self.entries.len() as i32 - 1) as usize;
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
                if self.current_entry().map(|e| e.kind == NodeKind::Dir).unwrap_or(false) {
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
        if idx < self.entries.len() {
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

impl<'a> StatefulWidget for PanelWidget<'a> {
    type State = PanelState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
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
            let text = if tagged_count > 0 {
                format!(" {}/{} tagged ", tagged_count, total)
            } else {
                format!(" {} entries ", total)
            };
            (text, Style::default().fg(border_color))
        };

        // 2 corners + 2 padding spaces around the title text itself.
        let title_max = (area.width as usize).saturating_sub(4);
        let title_text = truncate_path_front(&self.title, title_max);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" {} ", title_text),
                Style::default().fg(border_color),
            ))
            .title_bottom(Span::styled(footer_text, footer_style));

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
        // columns: tag(1) + space(1) + name + space(1) + size(8) + space(1) + date(time_length)
        let fixed = 1 + 1 + 1 + 8 + 1 + time_length;
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
            let name_label = if let Some(ref pat) = state.filter {
                format!("Name{} [{}]", icon(name_active), pat.raw)
            } else {
                format!("Name{}", icon(name_active))
            };
            let name_hdr = truncate_str(&name_label, name_width);
            let size_label = format!("  Size{} ", icon(size_active));  // 8 chars
            let size_hdr = format!("{:<8}", size_label);
            let date_label = format!(" Mtime{}", icon(date_active));
            let date_hdr = truncate_str(&date_label, time_length);
            let hdr_text = format!("  {} {} {}", name_hdr, size_hdr, date_hdr);
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
                    "{} {} {} {}",
                    tag_ch, name_part, size_part, date_part
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
}
