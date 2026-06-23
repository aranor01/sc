use crate::config::ColorScheme;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

pub struct CmdLineState {
    pub text: String,
    pub cursor: usize, // byte offset
    pub kill_ring: String,
}

impl CmdLineState {
    pub fn new() -> Self {
        CmdLineState {
            text: String::new(),
            cursor: 0,
            kill_ring: String::new(),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find the previous char boundary
            let mut pos = self.cursor - 1;
            while !self.text.is_char_boundary(pos) {
                pos -= 1;
            }
            self.text.drain(pos..self.cursor);
            self.cursor = pos;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.text.len() {
            let mut end = self.cursor + 1;
            while !self.text.is_char_boundary(end) {
                end += 1;
            }
            self.text.drain(self.cursor..end);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            let mut pos = self.cursor - 1;
            while !self.text.is_char_boundary(pos) {
                pos -= 1;
            }
            self.cursor = pos;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            let mut pos = self.cursor + 1;
            while pos <= self.text.len() && !self.text.is_char_boundary(pos) {
                pos += 1;
            }
            self.cursor = pos;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    // Returns the byte offset of the start of the word to the left of the cursor.
    // Skips spaces, then skips non-spaces (moving leftward).
    fn prev_word_boundary(&self) -> usize {
        let chars: Vec<(usize, char)> = self.text[..self.cursor].char_indices().collect();
        let mut i = chars.len();
        while i > 0 && chars[i - 1].1 == ' ' { i -= 1; }
        while i > 0 && chars[i - 1].1 != ' ' { i -= 1; }
        if i == 0 { 0 } else { chars[i].0 }
    }

    // Returns the byte offset of the end of the next word to the right of the cursor.
    // Skips spaces, then skips non-spaces (moving rightward).
    fn next_word_boundary(&self) -> usize {
        let chars: Vec<(usize, char)> = self.text[self.cursor..].char_indices().collect();
        let mut i = 0;
        while i < chars.len() && chars[i].1 == ' ' { i += 1; }
        while i < chars.len() && chars[i].1 != ' ' { i += 1; }
        if i == chars.len() { self.text.len() } else { self.cursor + chars[i].0 }
    }

    pub fn move_word_left(&mut self) {
        self.cursor = self.prev_word_boundary();
    }

    pub fn move_word_right(&mut self) {
        self.cursor = self.next_word_boundary();
    }

    pub fn kill_to_end(&mut self) {
        let killed: String = self.text.drain(self.cursor..).collect();
        if !killed.is_empty() { self.kill_ring = killed; }
    }

    pub fn kill_to_start(&mut self) {
        let killed: String = self.text.drain(..self.cursor).collect();
        if !killed.is_empty() { self.kill_ring = killed; }
        self.cursor = 0;
    }

    pub fn kill_word_left(&mut self) {
        let boundary = self.prev_word_boundary();
        let killed: String = self.text.drain(boundary..self.cursor).collect();
        if !killed.is_empty() { self.kill_ring = killed; }
        self.cursor = boundary;
    }

    pub fn kill_word_right(&mut self) {
        let boundary = self.next_word_boundary();
        let killed: String = self.text.drain(self.cursor..boundary).collect();
        if !killed.is_empty() { self.kill_ring = killed; }
    }

    pub fn yank(&mut self) {
        let s = self.kill_ring.clone();
        if !s.is_empty() { self.insert_str(&s); }
    }

    /// Return the display column of the cursor (byte offset == char offset for ASCII).
    pub fn display_cursor_col(&self) -> u16 {
        self.text[..self.cursor].chars().count() as u16
    }
}

use super::to_color;

pub struct CmdLineWidget<'a> {
    pub cs: &'a ColorScheme,
    pub prompt: &'a str,
    pub active: bool,
}

impl<'a> CmdLineWidget<'a> {
    pub fn render_with_cursor(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &CmdLineState,
    ) -> Option<Position> {
        let (fg, bg) = if self.active {
            (to_color(self.cs.cmdline_fg), to_color(self.cs.cmdline_bg))
        } else {
            (to_color(self.cs.cmdline_inactive_fg), to_color(self.cs.cmdline_inactive_bg))
        };
        let style = Style::default().fg(fg).bg(bg);

        let prompt_len = self.prompt.chars().count() as u16;
        let display = format!("{}{}", self.prompt, state.text);
        let para = Paragraph::new(Line::from(Span::styled(display, style)));
        Widget::render(para, area, buf);

        if area.height > 0 {
            let col = area.x + prompt_len + state.display_cursor_col();
            let col = col.min(area.x + area.width.saturating_sub(1));
            Some(Position { x: col, y: area.y })
        } else {
            None
        }
    }
}
