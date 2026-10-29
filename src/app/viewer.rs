use super::*;

impl App {
    pub(super) fn is_current_entry_text_file(&self) -> bool {
        let panel = self.active_panel();
        let Some(entry) = panel.current_entry() else { return false };
        if entry.kind != NodeKind::File {
            return false;
        }
        let abs = panel.provider.join(&panel.path, &entry.name).0;
        is_text_file(&abs)
    }

    /// Opens the active panel's current entry in the internal viewer (Shift-F3, and
    /// the `:view` internal default-file-action command). On a `Matches` panel this is
    /// the same as pressing Enter there (jump to the matched line, highlighted); on a
    /// `Dir`/`SearchResults` panel it shows the whole file with no highlight.
    pub(super) fn view_current_entry(&mut self, noop_because_it_is_binary: Option<&mut bool>) {
        if matches!(self.active_panel().content, PanelContent::Matches(_)) {
            self.open_match_in_viewer();
            return;
        }
        let panel = self.active_panel();
        let Some(entry) = panel.current_entry() else { return };
        if entry.kind == NodeKind::Dir {
            self.set_status("Cannot view a directory", true);
            return;
        }
        let abs = panel.provider.join(&panel.path, &entry.name).0;
        self.open_file_in_viewer(&abs, None, None, noop_because_it_is_binary);
    }

    /// Enter on a matching line: open the text viewer on that file, jumped to
    /// that line with the matches highlighted.
    pub(super) fn open_match_in_viewer(&mut self) {
        let (abs, needle, case_sensitive, content_is_regex, content_whole_words, line) = {
            let panel = self.active_panel();
            let PanelContent::Matches(ms) = &panel.content else { return };
            let Some(abs) = ms.abs_path.clone() else { return };
            let line = ms.matches.get(panel.cursor).map(|m| m.line).unwrap_or(1);
            (abs, ms.needle.clone(), ms.case_sensitive, ms.content_is_regex, ms.content_whole_words, line)
        };
        let highlight = Some((needle, case_sensitive, content_is_regex, content_whole_words));
        self.open_file_in_viewer(&abs, highlight, Some(line), None);
    }

    /// Reads `abs` and shows it in the internal viewer. `highlight`, if set, is the
    /// (needle, case_sensitive, is_regex, whole_words) tuple used to highlight matches
    /// (content-search jumps); `jump_line` scrolls to that 1-based line.
    /// `noop_because_it_is_binary` do nothing if not None and the first block contains
    /// a null character, and it's content is set to the result of the check
    pub(super) fn open_file_in_viewer(
        &mut self,
        abs: &str,
        highlight: Option<(String, bool, bool, bool)>,
        jump_line: Option<u64>,
        noop_because_it_is_binary: Option<&mut bool>,
    ) {
        match try_open_text_file(abs, noop_because_it_is_binary) {
            Ok(Some(bytes)) => {
                self.show_output = false;
                self.viewer = Some(ViewerContent {
                    title: format!(" {} (Esc to close) ", display_path(abs)),
                    text: normalize_text_file(&bytes),
                    highlight,
                });
                self.overlay.reset_scroll();
                if let Some(line) = jump_line {
                    self.overlay.jump_to_line(line);
                }
            }
            Ok(None) => {} // binary file detected, nothing to do
            Err(e) => self.set_status(&format!("Cannot open {}: {}", abs, e), true),
        }
    }
}

pub(super) fn shell_escape_path(s: &str) -> String {
    crate::macros::shell_escape(s)
}

// OSC alternative is first so \x1b] is consumed with its body;
// otherwise the fallback 2-char match would consume just \x1b].
// [0-?] adds DEC private sequences (\x1b=, \x1b>, \x1b7, \x1b8, etc.) missing from the
// original [@-Z\\-_] range (0x40-0x5F).
pub(super) static ANSI_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\x1b(?:\][^\x07\x1b]*(?:\x07|\x1b\\)|[0-?@-Z\\-_]|\[[0-?]*[ -/]*[@-~])",
    )
    .unwrap()
});

pub(super) fn normalize_pty_output(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    // PTY converts \n → \r\n; remove the bare \r.
    let s = s.replace('\r', "");
    let s = ANSI_RE.replace_all(&s, "");
    let tab_count = s.bytes().filter(|&b| b == b'\t').count();
    let mut out = String::with_capacity(s.len() + tab_count * 7);
    let mut col: usize = 0;
    const SPACES: &str = "        "; // 8 spaces — max tab expansion
    for ch in s.chars() {
        match ch {
            '\t' => {
                let spaces = 8 - (col % 8);
                out.push_str(&SPACES[..spaces]);
                col += spaces;
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            c if (c as u32) < 0x20 || c == '\x7f' => {}
            _ => {
                out.push(ch);
                col += ch.width().unwrap_or(1);
            }
        }
    }
    out
}

pub(super) fn is_bidi_or_invisible(c: char) -> bool {
    crate::ui::is_format_char(c)
}

pub(super) fn normalize_text_file(bytes: &[u8]) -> String {
        let s = String::from_utf8_lossy(bytes);
        let s = s.replace('\r', "");
        let s = ANSI_RE.replace_all(&s, |caps: &regex::Captures| {
            ".".repeat(caps.get(0).unwrap().as_str().chars().count())
        });
        let mut out = String::with_capacity(s.len());
        for ch in s.chars() {
            match ch {
                '\n' | '\t' => out.push(ch),
                c if c.is_control() => out.push('.'),
                c if is_bidi_or_invisible(c) => {}, // Unicode bidi/zero-width
                _ => out.push(ch),
            }
        }
        out
}

pub(super) fn try_open_text_file(
    abs: &str,
    noop_because_it_is_binary: Option<&mut bool>,
) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::{BufRead, Read};

    let mut reader = std::io::BufReader::new(std::fs::File::open(abs)?);

    if let Some(flag) = noop_because_it_is_binary {
        if reader.fill_buf()?.contains(&0u8) {
            *flag = true;
            return Ok(None);
        }
    }

    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

pub(super) fn is_text_file(abs: &str) -> bool {
    let mut noop_because_it_is_binary:Option<&mut bool> = Some(&mut false);
    _ = try_open_text_file(abs, noop_because_it_is_binary.as_deref_mut());
    !*noop_because_it_is_binary.unwrap()
}
