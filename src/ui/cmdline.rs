use crate::config::ColorScheme;
use crate::ui::modal_event::CmdlineOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
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

    pub fn handle_key(&mut self, event: &KeyEvent) -> CmdlineOutcome {
        match event.code {
            KeyCode::Char(c)
                if event.modifiers == KeyModifiers::NONE
                    || event.modifiers == KeyModifiers::SHIFT =>
            {
                self.insert_char(c);
                CmdlineOutcome::Consumed
            }
            KeyCode::Backspace if event.modifiers == KeyModifiers::NONE => {
                self.backspace();
                CmdlineOutcome::Consumed
            }
            KeyCode::Left if event.modifiers == KeyModifiers::NONE => {
                self.move_left();
                CmdlineOutcome::Consumed
            }
            KeyCode::Right if event.modifiers == KeyModifiers::NONE => {
                self.move_right();
                CmdlineOutcome::Consumed
            }
            KeyCode::Home if event.modifiers == KeyModifiers::NONE => {
                self.move_home();
                CmdlineOutcome::Consumed
            }
            KeyCode::End if event.modifiers == KeyModifiers::NONE => {
                self.move_end();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('a') if event.modifiers == KeyModifiers::CONTROL => {
                self.move_home();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('e') if event.modifiers == KeyModifiers::CONTROL => {
                self.move_end();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('k') if event.modifiers == KeyModifiers::CONTROL => {
                self.kill_to_end();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('u') if event.modifiers == KeyModifiers::CONTROL => {
                self.kill_to_start();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('w') if event.modifiers == KeyModifiers::CONTROL => {
                self.kill_word_left();
                CmdlineOutcome::Consumed
            }
            KeyCode::Backspace if event.modifiers == KeyModifiers::ALT => {
                self.kill_word_left();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('d') if event.modifiers == KeyModifiers::ALT => {
                self.kill_word_right();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('f') if event.modifiers == KeyModifiers::ALT => {
                self.move_word_right();
                CmdlineOutcome::Consumed
            }
            KeyCode::Left if event.modifiers == KeyModifiers::CONTROL => {
                self.move_word_left();
                CmdlineOutcome::Consumed
            }
            KeyCode::Right if event.modifiers == KeyModifiers::CONTROL => {
                self.move_word_right();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('y') if event.modifiers == KeyModifiers::CONTROL => {
                self.yank();
                CmdlineOutcome::Consumed
            }
            KeyCode::Char('p') if event.modifiers == KeyModifiers::CONTROL => CmdlineOutcome::HistoryPrev,
            KeyCode::Char('n') if event.modifiers == KeyModifiers::CONTROL => CmdlineOutcome::HistoryNext,
            _ => CmdlineOutcome::Passthrough,
        }
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
    /// How many terminal rows this cmdline needs at the given width.
    /// Always at least 1 (even when text is empty, the prompt occupies a row).
    pub fn needed_lines(&self, state: &CmdLineState, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }
        let total = self.prompt.chars().count() + state.text.chars().count();
        total.max(1).div_ceil(width as usize) as u16
    }

    pub fn render_with_cursor(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &CmdLineState,
    ) -> Option<Position> {
        if area.height == 0 || area.width == 0 {
            return None;
        }

        let (fg, bg) = if self.active {
            (to_color(self.cs.cmdline_fg), to_color(self.cs.cmdline_bg))
        } else {
            (to_color(self.cs.cmdline_inactive_fg), to_color(self.cs.cmdline_inactive_bg))
        };
        let style = Style::default().fg(fg).bg(bg);

        let width = area.width as usize;
        let prompt_len = self.prompt.chars().count();
        let all_chars: Vec<char> = self.prompt.chars().chain(state.text.chars()).collect();

        // Fill entire area with the cmdline background first
        let blank = " ".repeat(width);
        for row in 0..area.height {
            buf.set_string(area.x, area.y + row, &blank, style);
        }

        // Render character chunks, one per visual row (character-level wrapping)
        for (row_idx, chunk) in all_chars.chunks(width).enumerate() {
            let y = area.y + row_idx as u16;
            if y >= area.y + area.height {
                break;
            }
            let s: String = chunk.iter().collect();
            buf.set_string(area.x, y, &s, style);
        }

        // Cursor position: split total char offset into (row, col)
        let cursor_total = prompt_len + state.display_cursor_col() as usize;
        let cursor_row = (cursor_total / width) as u16;
        let cursor_col = (cursor_total % width) as u16;
        let pos = Position { x: area.x + cursor_col, y: area.y + cursor_row };

        if pos.y < area.y + area.height {
            Some(pos)
        } else {
            None
        }
    }
}
