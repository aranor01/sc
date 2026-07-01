use crate::config::ColorScheme;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
};

use super::to_color;

/// A pressable labeled button that carries its own color pair.
///
/// `area` is derived from the label width at construction; `fg`/`bg` are the
/// normal (unpressed) colors — they are inverted when the button is pressed.
#[derive(Clone, Copy)]
pub struct Button {
    pub area: Rect,
    fg: Color,
    bg: Color,
    access_key: Option<char>,
}

impl Default for Button {
    fn default() -> Self {
        // zero area → contains() always returns false (safe sentinel value)
        Button { area: Rect::default(), fg: Color::Reset, bg: Color::Reset, access_key: None }
    }
}

impl Button {
    /// Build a dialog button using the color scheme's `dialog_butt_fg/bg`.
    pub fn build(label: &str, x: u16, y: u16, cs: &ColorScheme) -> Self {
        Button {
            area: Rect { x, y, width: label.len() as u16, height: 1 },
            fg: to_color(cs.dialog_butt_fg),
            bg: to_color(cs.dialog_butt_bg),
            access_key: None,
        }
    }

    /// Like `build`, but the first occurrence of `access_key` in `label` is
    /// rendered bold and underlined (e.g. to mark a mnemonic/hotkey letter).
    pub fn build_with_access_key(label: &str, x: u16, y: u16, cs: &ColorScheme, access_key: char) -> Self {
        Button {
            area: Rect { x, y, width: label.len() as u16, height: 1 },
            fg: to_color(cs.dialog_butt_fg),
            bg: to_color(cs.dialog_butt_bg),
            access_key: Some(access_key),
        }
    }

    pub fn build_with_colors(label: &str, x: u16, y: u16, fg: Color, bg: Color) -> Self {
        Button {
            area: Rect { x, y, width: label.len() as u16, height: 1 },
            fg,
            bg,
            access_key: None,
        }
    }

    pub fn contains(self, pos: Position) -> bool {
        self.area.contains(pos)
    }

    pub fn is_pressed(self, press: Option<Position>) -> bool {
        press.map(|p| self.area.contains(p)).unwrap_or(false)
    }

    /// True when `down` and `up` are the same cell and `up` is inside this button.
    /// Combines the "Down+Up on same cell" and "hit test" checks into one call.
    pub fn clicked(self, down: Option<Position>, up: Position) -> bool {
        down == Some(up) && self.is_pressed(Some(up))
    }

    /// Render `label` at the button's position, detecting pressed from `press`.
    pub fn render(self, label: &str, buf: &mut Buffer, press: Option<Position>) {
        self.render_state(label, buf, self.is_pressed(press));
    }

    /// Render `label` with an externally computed `pressed` flag.
    /// Use this when pressed detection must cover a wider area than self.area.
    pub fn render_state(self, label: &str, buf: &mut Buffer, pressed: bool) {
        let style = if pressed {
            Style::default().fg(self.bg).bg(self.fg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.fg).bg(self.bg).add_modifier(Modifier::BOLD)
        };
        match self.access_key.and_then(|c| label.find(c)) {
            Some(idx) => {
                let (before, rest) = label.split_at(idx);
                let mut chars = rest.chars();
                let key_char = chars.next().unwrap();
                let after = chars.as_str();
                let mut x = self.area.x;
                buf.set_string(x, self.area.y, before, style);
                x += before.chars().count() as u16;
                buf.set_string(
                    x,
                    self.area.y,
                    key_char.to_string(),
                    style.add_modifier(Modifier::UNDERLINED),
                );
                x += 1;
                buf.set_string(x, self.area.y, after, style);
            }
            None => buf.set_string(self.area.x, self.area.y, label, style),
        }
    }
}
