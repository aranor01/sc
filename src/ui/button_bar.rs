use crate::config::{ActionBindings, ColorScheme, KeyBinding, KeyBindings};
use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

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
}

impl<'a> Widget for ButtonBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let butt_normal = Style::default()
            .fg(to_color(self.cs.button_bar_butt_fg))
            .bg(to_color(self.cs.button_bar_butt_bg));
        let butt_pressed = Style::default()
            .fg(to_color(self.cs.button_bar_butt_bg))
            .bg(to_color(self.cs.button_bar_butt_fg));
        let label_normal = Style::default()
            .fg(to_color(self.cs.button_bar_fg))
            .bg(to_color(self.cs.button_bar_bg));
        let label_pressed = Style::default()
            .fg(to_color(self.cs.button_bar_bg))
            .bg(to_color(self.cs.button_bar_fg));

        // Only the column matters for the button bar (it occupies a single row).
        let pressed_col: Option<u16> = self.press
            .filter(|p| p.y == area.y)
            .map(|p| p.x);

        let buttons = Self::buttons(self.kb);
        let mut spans = Vec::new();
        let mut x = area.x;
        for (n, label) in &buttons {
            let fkey_str = format!("F{}", n);
            let label_str = format!("{} ", label);
            let fkey_len = fkey_str.len() as u16;
            let label_len = label_str.len() as u16;
            let button_end = x + fkey_len + label_len;
            let pressed = pressed_col.map(|c| c >= x && c < button_end).unwrap_or(false);
            spans.push(Span::styled(fkey_str, if pressed { butt_pressed } else { butt_normal }));
            spans.push(Span::styled(label_str, if pressed { label_pressed } else { label_normal }));
            x += fkey_len + label_len;
        }

        let line = Line::from(spans);
        let para = Paragraph::new(line).style(Style::default().bg(to_color(self.cs.button_bar_bg)));
        Widget::render(para, area, buf);
    }
}
