use crate::config::{format_key, ActionBindings, ColorScheme, KeyBinding, MenuItem};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    widgets::Widget,
};

use super::button::Button;
use super::to_color;

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

/// Short name used on the bar when the CheatSheet has no label for a built-in action.
pub fn fallback_label(config_key: &str) -> Option<&'static str> {
    Some(match config_key {
        "action_palette" => "Actions",
        "user_menu" => "Menu",
        "view" => "View",
        "copy" => "Copy",
        "move" => "Move",
        "mkdir" => "Mkdir",
        "delete" => "Delete",
        "exit" => "Quit",
        _ => return None,
    })
}

/// What happens when a button bar entry is clicked.
#[derive(Clone)]
pub enum BarAction {
    Fkey(u8),
    Menu(String),
}

/// A single button bar entry: a key label + human label.
#[derive(Clone)]
pub struct BarEntry {
    pub key_label: String,
    pub label: String,
    pub action: BarAction,
}

pub struct ButtonBarWidget<'a> {
    pub cs: &'a ColorScheme,
    /// Built-in actions as (display label, bindings).
    pub builtins: &'a [(String, &'a ActionBindings)],
    pub menu: &'a [MenuItem],
    pub press: Option<Position>,
}

impl<'a> ButtonBarWidget<'a> {
    /// Returns bar entries: built-in Fkey bindings, then menu Fkey items, both sorted by Fkey;
    /// then non-Fkey `add_to_bar` menu items in config order.
    pub fn entries(builtins: &[(String, &ActionBindings)], menu: &[MenuItem]) -> Vec<BarEntry> {
        let mut fkey_entries: Vec<(u8, BarEntry)> = Vec::new();
        let mut extra_entries: Vec<BarEntry> = Vec::new();

        // Built-in bindings
        for (label, bindings) in builtins {
            if let Some(n) = first_fkey(bindings) {
                fkey_entries.push((n, BarEntry {
                    key_label: format!("F{n}"),
                    label: label.clone(),
                    action: BarAction::Fkey(n),
                }));
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
                                fkey_entries.push((n, BarEntry {
                                    key_label: format!("F{n}"),
                                    label: item.label.clone(),
                                    action: BarAction::Fkey(n),
                                }));
                                continue;
                            }
                        }
                        // Non-Fkey (or modified-key) binding — run the command directly
                        // rather than reconstructing a fake KeyEvent that would lose
                        // whatever modifier it actually used.
                        extra_entries.push(BarEntry {
                            key_label: format_key(&k),
                            label: item.label.clone(),
                            action: BarAction::Menu(item.command.clone()),
                        });
                        continue;
                    }
                }
            }
            // No keys but add_to_bar — show label without key, still directly runnable.
            extra_entries.push(BarEntry {
                key_label: String::new(),
                label: item.label.clone(),
                action: BarAction::Menu(item.command.clone()),
            });
        }

        fkey_entries.sort_by_key(|(n, _)| *n);
        let mut entries: Vec<BarEntry> = fkey_entries.into_iter().map(|(_, e)| e).collect();
        entries.extend(extra_entries);
        entries
    }

    /// Returns the action of the button whose area contains `pos`, if any.
    pub fn button_at(builtins: &[(String, &ActionBindings)], menu: &[MenuItem], bb_area: Rect, pos: Position) -> Option<BarAction> {
        if pos.y != bb_area.y {
            return None;
        }
        let mut x = bb_area.x;
        for entry in Self::entries(builtins, menu) {
            let w = (entry.key_label.len() + entry.label.len() + 1) as u16;
            if pos.x >= x && pos.x < x + w {
                return Some(entry.action);
            }
            x += w;
            if x >= bb_area.x + bb_area.width {
                break;
            }
        }
        None
    }

}

impl<'a> Widget for ButtonBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let butt_fg = to_color(self.cs.button_bar_butt_fg);
        let butt_bg = to_color(self.cs.button_bar_butt_bg);
        let label_fg = to_color(self.cs.button_bar_fg);
        let label_bg = to_color(self.cs.button_bar_bg);

        let entries = Self::entries(self.builtins, self.menu);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_key_binding;

    #[test]
    fn builtin_with_custom_fkey_is_shown_with_its_label() {
        let sort = vec![parse_key_binding("F9").unwrap()];
        let ctrl = vec![parse_key_binding("Ctrl-s").unwrap()];
        let builtins = vec![("Sort Panel".to_string(), &sort), ("Other".to_string(), &ctrl)];
        let entries = ButtonBarWidget::entries(&builtins, &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].key_label.as_str(), entries[0].label.as_str()), ("F9", "Sort Panel"));
    }
}
