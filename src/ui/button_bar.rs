use crate::config::{ActionBindings, ColorScheme, KeyBinding, KeyBindings};
use crossterm::event::KeyCode;
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
                return Some(n);
            }
        }
    }
    None
}

pub struct ButtonBarWidget<'a> {
    pub cs: &'a ColorScheme,
    pub kb: &'a KeyBindings,
    pub press: Option<Position>,
}

impl<'a> ButtonBarWidget<'a> {
    /// Returns list of (fkey_num, label) in order.
    pub fn buttons(kb: &KeyBindings) -> Vec<(u8, &'static str)> {
        let mut items: Vec<(u8, &'static str)> = Vec::new();
        let pairs: &[(&ActionBindings, &'static str)] = &[
            (&kb.user_menu, "Menu"),
            (&kb.copy, "Copy"),
            (&kb.move_entry, "Move"),
            (&kb.delete, "Delete"),
            (&kb.exit, "Quit"),
        ];
        for (bindings, label) in pairs {
            if let Some(n) = first_fkey(bindings) {
                items.push((n, label));
            }
        }
        items.sort_by_key(|(n, _)| *n);
        items
    }

    /// Returns the F-key number of the button whose area contains `pos`.
    pub fn button_at(kb: &KeyBindings, bb_area: Rect, pos: Position) -> Option<u8> {
        if pos.y != bb_area.y {
            return None;
        }
        let mut x = bb_area.x;
        for (n, label) in Self::buttons(kb) {
            let w = (format!("F{}", n).len() + label.len() + 1) as u16;
            if pos.x >= x && pos.x < x + w {
                return Some(n);
            }
            x += w;
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

        let buttons = Self::buttons(self.kb);
        let mut x = area.x;
        for (n, label) in &buttons {
            let fkey_str = format!("F{}", n);
            let label_str = format!("{} ", label);
            let fkey_len = fkey_str.len() as u16;
            let label_len = label_str.len() as u16;

            let pressed = self.press
                .map(|p| p.y == area.y && p.x >= x && p.x < x + fkey_len + label_len)
                .unwrap_or(false);

            // Fn number: rendered directly with button_bar_butt colors
            let fkey_style = if pressed {
                Style::default().fg(butt_bg).bg(butt_fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(butt_fg).bg(butt_bg).add_modifier(Modifier::BOLD)
            };
            buf.set_string(x, area.y, &fkey_str, fkey_style);

            // Label: rendered via Button with button_bar label colors
            Button::build_with_colors(&label_str, x + fkey_len, area.y, label_fg, label_bg)
                .render_state(&label_str, buf, pressed);

            x += fkey_len + label_len;
        }

        // Fill the rest of the row with the button bar background
        if x < area.x + area.width {
            let fill = " ".repeat((area.x + area.width - x) as usize);
            buf.set_string(x, area.y, &fill, Style::default().bg(label_bg));
        }
    }
}
