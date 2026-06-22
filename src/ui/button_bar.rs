use crate::config::{ActionBindings, ColorScheme, KeyBinding, KeyBindings};
use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
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
}

impl<'a> ButtonBarWidget<'a> {
    /// Returns list of (label, fkey_num) in order.
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
        let butt_style = Style::default()
            .fg(to_color(self.cs.button_bar_butt_fg))
            .bg(to_color(self.cs.button_bar_butt_bg));
        let label_style = Style::default()
            .fg(to_color(self.cs.button_bar_fg))
            .bg(to_color(self.cs.button_bar_bg));

        let buttons = Self::buttons(self.kb);
        let mut spans = Vec::new();
        for (n, label) in &buttons {
            spans.push(Span::styled(format!("F{}", n), butt_style));
            spans.push(Span::styled(format!("{} ", label), label_style));
        }

        // Fill remainder
        let line = Line::from(spans);
        let para = Paragraph::new(line).style(Style::default().bg(to_color(self.cs.button_bar_bg)));
        Widget::render(para, area, buf);
    }
}
