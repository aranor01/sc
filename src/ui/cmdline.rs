use crate::config::ColorScheme;
use crate::ui::modal_event::CmdlineOutcome;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};

// Unicode's explicit bidirectional-control characters: the embedding/
// override/isolate pairs plus the two directional marks. None of these are
// covered by `char::is_control()` below (Unicode classifies them as
// ordinary, zero-width "format" characters, not control characters), but
// they let the *visual* order of the surrounding text diverge from its
// actual left-to-right byte order — the same "Trojan Source" technique used
// to make source code or file names display as something other than what
// they are. In a command line that's about to be handed to a shell, that's
// a direct visual-spoofing risk: text can be made to *look* different from
// what will actually execute when Enter is pressed. There is no legitimate
// reason for a shell command or a file name to require one of these, so
// they're blocked outright rather than merely discouraged.
const BIDI_CONTROL_CHARS: [char; 11] = [
    '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', // LRE, RLE, PDF, LRO, RLO
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',             // LRI, RLI, FSI, PDI
    '\u{200E}', '\u{200F}',                                     // LRM, RLM
];

/// Whether `c` is safe to store in the command-line buffer.
///
/// This is the single chokepoint every source of command-line text funnels
/// through — normal typing, autocomplete, yanking previously-killed text,
/// copying a file/path name in with Alt-Enter or Ctrl-x-t, and text injected
/// over IPC via `InjectToCommandLine` (see docs/IpcActions.md) — because all
/// of them ultimately call `insert_char`/`insert_str` below. That matters
/// because more than one of those sources can carry attacker-influenced
/// bytes: a file name on Linux may contain any byte except NUL and `/`,
/// including raw control characters, and `InjectToCommandLine`'s text comes
/// straight from whatever connected to the IPC socket (see ipc.rs's
/// `SO_PEERCRED` check for who that can be).
///
/// Two things are rejected:
///
/// - **Unicode control characters** (`char::is_control()`: C0 0x00-0x1F, DEL
///   0x7F, C1 0x80-0x9F). This is what actually matters most: every ANSI/CSI/
///   OSC terminal escape sequence begins with the ESC control character
///   (0x1B), and the command line is rendered straight to the real terminal
///   (`CmdLineWidget::render_with_cursor` below writes it into the Ratatui
///   buffer verbatim, which the terminal backend then prints as-is). If ESC
///   — or any other control byte — can never be stored here, no escape
///   sequence can ever be assembled from this buffer's contents, full stop,
///   regardless of what would otherwise follow it. Without this, a
///   maliciously-named file or a hostile IPC caller could send raw escape
///   sequences straight through to whatever terminal emulator is hosting
///   `sc`.
/// - **Bidi control characters** (`BIDI_CONTROL_CHARS` above): see its own
///   comment.
///
/// Everything else passes through untouched — this is not an ASCII filter;
/// every printable character in every script, including multi-byte UTF-8,
/// is left alone.
fn is_cmdline_safe(c: char) -> bool {
    !c.is_control() && !BIDI_CONTROL_CHARS.contains(&c)
}

#[derive(Debug, Clone)]
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
        if !is_cmdline_safe(c) {
            return;
        }
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        // Filter rather than reject outright: a mostly-safe string with one
        // stray disallowed character (e.g. a file name someone accidentally
        // gave a stray control byte) should still insert the rest, the same
        // way a single disallowed keystroke is simply dropped in
        // `insert_char` above rather than discarding whatever else the user
        // was typing.
        let filtered: String = s.chars().filter(|&c| is_cmdline_safe(c)).collect();
        self.text.insert_str(self.cursor, &filtered);
        self.cursor += filtered.len();
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
            KeyCode::Delete if event.modifiers == KeyModifiers::NONE => {
                self.delete_char();
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
            KeyCode::Char('b') if event.modifiers == KeyModifiers::ALT => {
                self.move_word_left();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_str_strips_control_characters() {
        let mut s = CmdLineState::new();
        s.insert_str("safe\x1b[31mtext");
        assert_eq!(s.text, "safe[31mtext");
    }

    #[test]
    fn insert_str_strips_bidi_override() {
        let mut s = CmdLineState::new();
        s.insert_str("a\u{202E}b");
        assert_eq!(s.text, "ab");
    }

    #[test]
    fn insert_str_keeps_printable_unicode() {
        let mut s = CmdLineState::new();
        s.insert_str("héllo 世界 🎉");
        assert_eq!(s.text, "héllo 世界 🎉");
    }

    #[test]
    fn insert_str_advances_cursor_by_filtered_length_not_original() {
        let mut s = CmdLineState::new();
        s.insert_str("a\x1bb");
        assert_eq!(s.text, "ab");
        assert_eq!(s.cursor, s.text.len());
    }

    #[test]
    fn insert_char_drops_control_character() {
        let mut s = CmdLineState::new();
        s.insert_char('a');
        s.insert_char('\x1b');
        s.insert_char('b');
        assert_eq!(s.text, "ab");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn insert_char_keeps_printable_char() {
        let mut s = CmdLineState::new();
        s.insert_char('x');
        assert_eq!(s.text, "x");
        assert_eq!(s.cursor, 1);
    }
}
