use crate::config::{format_key, ActionBindings, ColorScheme, KeyBinding, KeyBindings, MenuItem};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    widgets::Widget,
};

use super::button::Button;
use super::to_color;

fn first_key(bindings: &ActionBindings) -> Option<KeyEvent> {
    for b in bindings {
        if let KeyBinding::Single(ke) = b {
            return Some(*ke);
        }
    }
    None
}

fn first_fkey(bindings: &ActionBindings) -> Option<u8> {
    for b in bindings {
        if let KeyBinding::Single(ke) = b {
            if let KeyCode::F(n) = ke.code {
                if ke.modifiers == KeyModifiers::NONE {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// A single button bar entry: a key label + human label.
#[derive(Clone)]
pub struct BarEntry {
    pub key_label: String,
    pub label: String,
    pub fkey: Option<u8>,
}

pub struct ButtonBarWidget<'a> {
    pub cs: &'a ColorScheme,
    pub kb: &'a KeyBindings,
    pub menu: &'a [MenuItem],
    pub press: Option<Position>,
}

impl<'a> ButtonBarWidget<'a> {
    /// Returns bar entries: built-in Fkey bindings, then menu Fkey items, both sorted by Fkey;
    /// then non-Fkey `add_to_bar` menu items in config order.
    pub fn entries(kb: &KeyBindings, menu: &[MenuItem]) -> Vec<BarEntry> {
        let mut fkey_entries: Vec<BarEntry> = Vec::new();
        let mut extra_entries: Vec<BarEntry> = Vec::new();

        // Built-in bindings
        let pairs: &[(&ActionBindings, &str)] = &[
            (&kb.user_menu,   "Menu"),
            (&kb.copy,        "Copy"),
            (&kb.move_entry,  "Move"),
            (&kb.delete,      "Delete"),
            (&kb.exit,        "Quit"),
        ];
        for (bindings, label) in pairs {
            if let Some(n) = first_fkey(bindings) {
                fkey_entries.push(BarEntry {
                    key_label: format!("F{n}"),
                    label: label.to_string(),
                    fkey: Some(n),
                });
            }
        }

        // Menu items with add_to_bar
        for item in menu {
            if !item.add_to_bar { continue; }
            if let Some(keys_str) = &item.keys {
                if let Ok(binding) = crate::config::parse_key_binding(keys_str) {
                    let ke = match &binding {
                        KeyBinding::Single(k) => Some(*k),
                        KeyBinding::Chord(f, _) => Some(*f),
                    };
                    if let Some(k) = ke {
                        if let KeyCode::F(n) = k.code {
                            if k.modifiers == KeyModifiers::NONE {
                                fkey_entries.push(BarEntry {
                                    key_label: format!("F{n}"),
                                    label: item.label.clone(),
                                    fkey: Some(n),
                                });
                                continue;
                            }
                        }
                        // Non-Fkey binding
                        extra_entries.push(BarEntry {
                            key_label: format_key(&k),
                            label: item.label.clone(),
                            fkey: None,
                        });
                        continue;
                    }
                }
            }
            // No keys but add_to_bar — show label without key
            extra_entries.push(BarEntry {
                key_label: String::new(),
                label: item.label.clone(),
                fkey: None,
            });
        }

        fkey_entries.sort_by_key(|e| e.fkey);
        fkey_entries.extend(extra_entries);
        fkey_entries
    }

    /// Returns the Fkey number (or None for non-Fkey entries) whose area contains `pos`.
    pub fn button_at(kb: &KeyBindings, menu: &[MenuItem], bb_area: Rect, pos: Position) -> Option<u8> {
        if pos.y != bb_area.y {
            return None;
        }
        let mut x = bb_area.x;
        for entry in Self::entries(kb, menu) {
            let w = (entry.key_label.len() + entry.label.len() + 1) as u16;
            if pos.x >= x && pos.x < x + w {
                return entry.fkey;
            }
            x += w;
            if x >= bb_area.x + bb_area.width {
                break;
            }
        }
        None
    }

    // Legacy: only built-in Fkey buttons (used by callers that don't have menu yet)
    pub fn buttons(kb: &KeyBindings) -> Vec<(u8, &'static str)> {
        let mut items: Vec<(u8, &'static str)> = Vec::new();
        let pairs: &[(&ActionBindings, &'static str)] = &[
            (&kb.user_menu,  "Menu"),
            (&kb.copy,       "Copy"),
            (&kb.move_entry, "Move"),
            (&kb.delete,     "Delete"),
            (&kb.exit,       "Quit"),
        ];
        for (bindings, label) in pairs {
            if let Some(n) = first_fkey(bindings) {
                items.push((n, label));
            }
        }
        items.sort_by_key(|(n, _)| *n);
        items
    }
}

impl<'a> Widget for ButtonBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let butt_fg = to_color(self.cs.button_bar_butt_fg);
        let butt_bg = to_color(self.cs.button_bar_butt_bg);
        let label_fg = to_color(self.cs.button_bar_fg);
        let label_bg = to_color(self.cs.button_bar_bg);

        let entries = Self::entries(self.kb, self.menu);
        let mut x = area.x;
        for entry in &entries {
            let key_str = &entry.key_label;
            let label_str = format!("{} ", entry.label);
            let key_len = key_str.len() as u16;
            let label_len = label_str.len() as u16;
            let total = key_len + label_len;

            if x + total > area.x + area.width {
                break;
            }

            let pressed = self.press
                .map(|p| p.y == area.y && p.x >= x && p.x < x + total)
                .unwrap_or(false);

            if !key_str.is_empty() {
                let fkey_style = if pressed {
                    Style::default().fg(butt_bg).bg(butt_fg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(butt_fg).bg(butt_bg).add_modifier(Modifier::BOLD)
                };
                buf.set_string(x, area.y, key_str, fkey_style);
            }

            Button::build_with_colors(&label_str, x + key_len, area.y, label_fg, label_bg)
                .render_state(&label_str, buf, pressed);

            x += total;
        }

        if x < area.x + area.width {
            let fill = " ".repeat((area.x + area.width - x) as usize);
            buf.set_string(x, area.y, &fill, Style::default().bg(label_bg));
        }
    }
}
