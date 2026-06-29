use crate::config::ColorScheme;
use crate::provider::{NodeEntry, NodeKind, NodePath, TreeProvider};
use crate::ui::modal_event::PanelOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use chrono::{DateTime, Local};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
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

/// A compiled filter/select-group pattern. Patterns starting with `/` are regex;
/// others are shell globs.
#[derive(Debug, Clone)]
pub struct FilterPattern {
    pub raw: String,
    is_regex: bool,
    regex: Option<regex::Regex>,
    glob: Option<glob::Pattern>,
}

impl FilterPattern {
    pub fn matches(&self, name: &str) -> bool {
        if self.is_regex {
            self.regex.as_ref().map_or(false, |r| r.is_match(name))
        } else {
            self.glob.as_ref().map_or(false, |g| g.matches(name))
        }
    }
}

/// Validate and compile a filter/select pattern.
/// Returns `Err(description)` on invalid patterns.
pub fn validate_filter_pattern(input: &str) -> Result<FilterPattern, String> {
    if input.starts_with('/') {
        let re_src = &input[1..];
        match regex::Regex::new(re_src) {
            Ok(r) => Ok(FilterPattern { raw: input.to_string(), is_regex: true, regex: Some(r), glob: None }),
            Err(e) => Err(format!("Invalid regex: {e}")),
        }
    } else {
        match glob::Pattern::new(input) {
            Ok(g) => Ok(FilterPattern { raw: input.to_string(), is_regex: false, regex: None, glob: Some(g) }),
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
                        .filter(|e| e.name != ".." && !pat.matches(&e.name))
                        .map(|e| e.name.clone())
                        .collect();
                    self.tagged.retain(|n| !hidden.contains(n));
                    entries.retain(|e| e.name == ".." || pat.matches(&e.name));
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
                    .map_or(false, |io| io.kind() == std::io::ErrorKind::NotFound);
                self.error = Some(if is_not_found {
                    format!("directory no longer exists: {}", self.path.0)
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
        let Some(entry) = self.entries.get(self.cursor) else {
            return None;
        };
        let new_path = if entry.name == ".." {
            self.provider.parent(&self.path)?
        } else if entry.kind == NodeKind::Dir {
            self.provider.join(&self.path, &entry.name)
        } else {
            return None;
        };
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

fn format_date(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d").to_string()
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

pub struct PanelWidget<'a> {
    pub cs: &'a ColorScheme,
    pub active: bool,
    pub title: String,
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
            (format!(" {} ", err), Style::default().fg(Color::Red))
        } else {
            let total = state.entries.len();
            let text = if tagged_count > 0 {
                format!(" {}/{} tagged ", tagged_count, total)
            } else {
                format!(" {} entries ", total)
            };
            (text, Style::default().fg(border_color))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" {} ", self.title),
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
        // columns: tag(1) + space(1) + name + space(1) + size(8) + space(1) + date(10)
        let fixed = 1 + 1 + 1 + 8 + 1 + 10;
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
            let date_label = format!(" Mtime{}   ", icon(date_active));  // 10 chars
            let date_hdr = format!("{:<10}", date_label);
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
                    "          ".to_string()
                } else {
                    format_date(entry.modified)
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
