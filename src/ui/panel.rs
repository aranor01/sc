use crate::config::ColorScheme;
use crate::provider::{NodeEntry, NodeKind, NodePath, TreeProvider};
use chrono::{DateTime, Local};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use std::collections::HashSet;
use std::time::SystemTime;

pub struct PanelState {
    pub provider: Box<dyn TreeProvider>,
    pub path: NodePath,
    pub entries: Vec<NodeEntry>,
    pub cursor: usize,
    pub scroll: usize,
    pub tagged: HashSet<String>,
    pub error: Option<String>,
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
        };
        s.refresh();
        s
    }

    pub fn refresh(&mut self) {
        match self.provider.list(&self.path) {
            Ok(mut entries) => {
                entries.sort_by(|a, b| match (&a.kind, &b.kind) {
                    (NodeKind::Dir, NodeKind::File) => std::cmp::Ordering::Less,
                    (NodeKind::File, NodeKind::Dir) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                });
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
                self.entries = entries;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.entries.clear();
            }
        }
        if self.cursor >= self.entries.len() && !self.entries.is_empty() {
            self.cursor = self.entries.len() - 1;
        }
        if self.entries.is_empty() {
            self.cursor = 0;
            self.scroll = 0;
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

    pub fn enter_dir(&mut self) {
        let Some(entry) = self.entries.get(self.cursor) else {
            return;
        };
        if entry.name == ".." {
            if let Some(parent) = self.provider.parent(&self.path) {
                self.path = parent;
                self.cursor = 0;
                self.scroll = 0;
                self.tagged.clear();
                self.refresh();
            }
        } else if entry.kind == NodeKind::Dir {
            let new_path = self.provider.join(&self.path, &entry.name.clone());
            self.path = new_path;
            self.cursor = 0;
            self.scroll = 0;
            self.tagged.clear();
            self.refresh();
        }
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

    pub fn tag_toggle(&mut self, visible_height: usize) {
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
        self.move_cursor(1, visible_height);
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

fn to_color(c: crate::config::Color) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

fn format_size(bytes: u64, kind: &NodeKind) -> String {
    if *kind == NodeKind::Dir {
        return "   <DIR>".to_string();
    }
    if bytes < 1024 {
        format!("{:>7} B", bytes)
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
        let total = state.entries.len();
        let footer = if tagged_count > 0 {
            format!(" {}/{} tagged ", tagged_count, total)
        } else {
            format!(" {} entries ", total)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                format!(" {} ", self.title),
                Style::default().fg(border_color),
            ))
            .title_bottom(Span::styled(footer, Style::default().fg(border_color)));

        let inner = block.inner(area);
        block.render(area, buf);

        if let Some(err) = &state.error {
            let msg = format!("Error: {}", err);
            let line = Line::from(Span::styled(
                msg,
                Style::default().fg(Color::Red).bg(to_color(self.cs.panel_bg)),
            ));
            let p =
                ratatui::widgets::Paragraph::new(line).style(Style::default().bg(to_color(self.cs.panel_bg)));
            p.render(inner, buf);
            return;
        }

        let visible_height = inner.height as usize;
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

        StatefulWidget::render(list, inner, buf, &mut list_state);
    }
}
