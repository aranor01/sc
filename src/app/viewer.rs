use super::*;
use crate::textbuf::{looks_binary, ANSI_RE};

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

    pub(super) fn viewer_toggle_line_numbers(&mut self) {
        if self.viewer.is_none() {
            return;
        }
        self.overlay.clear_feedback();
        self.show_line_numbers = !self.show_line_numbers;
    }

    /// Opens the text search dialog over the viewer, prefilled with the last query (or
    /// the highlight a content-search jump opened the viewer with).
    pub(super) fn viewer_open_search_dialog(&mut self) {
        let Some((_, highlight)) =
            text_view(&mut self.viewer, self.show_output, &mut self.last_output, &mut self.output_highlight)
        else {
            return;
        };
        let highlight = highlight.clone();
        self.overlay.clear_feedback();
        let mut state = SearchDialogState::new_viewer_text();
        if let Some((text, case_sensitive, is_regex, whole_words)) =
            self.viewer_query.clone().or(highlight)
        {
            state.content.cursor = text.len();
            state.content.text = text;
            state.content_case_sensitive = case_sensitive;
            state.content_is_regex = is_regex;
            state.content_whole_words = whole_words;
        }
        self.modal = Modal::SearchDialog(state);
    }

    /// Confirms the viewer's search dialog: highlights the text and jumps to its first
    /// match at or after the top of the view. An empty text clears the highlight.
    pub(super) fn execute_viewer_search(&mut self, mut state: SearchDialogState) {
        let Some((_, highlight)) =
            text_view(&mut self.viewer, self.show_output, &mut self.last_output, &mut self.output_highlight)
        else {
            return;
        };
        let text = state.content.text.clone();
        if text.is_empty() {
            *highlight = None;
            self.viewer_query = None;
            return;
        }
        let query = (
            text,
            state.content_case_sensitive,
            state.content_is_regex,
            state.content_whole_words,
        );
        if let Err(e) = ContentMatcher::build(&query.0, query.2, query.1, query.3) {
            state.error = Some(e);
            self.modal = Modal::SearchDialog(state);
            return;
        }
        *highlight = Some(query.clone());
        self.viewer_query = Some(query);
        self.overlay.match_line = None; // a new search starts from the top of the view
        self.overlay.current_line = None;
        self.viewer_find(SearchDirection::Forward, true);
    }

    /// Next/previous line with a match of the viewer's highlighted text. Without a
    /// highlight there is nothing to look for yet, so the search dialog opens. When the
    /// previous search ran out of matches in this direction, searching again wraps around.
    pub(super) fn viewer_find(&mut self, direction: SearchDirection, inclusive: bool) {
        let Some((buf, highlight)) =
            text_view(&mut self.viewer, self.show_output, &mut self.last_output, &mut self.output_highlight)
        else {
            return;
        };
        let Some((needle, case_sensitive, is_regex, whole_words)) = highlight.clone() else {
            self.viewer_open_search_dialog();
            return;
        };
        let Ok(matcher) = ContentMatcher::build(&needle, is_regex, case_sensitive, whole_words) else {
            self.viewer_open_search_dialog();
            return;
        };
        let wrapping = self.overlay.wrap_armed == Some(direction);
        self.overlay.clear_feedback();

        let forward = direction == SearchDirection::Forward;
        let start = if wrapping {
            if forward { Some(0) } else { buf.total_lines().checked_sub(1) }
        } else {
            match (self.overlay.match_line, forward) {
                (Some(line), true) => Some(line + 1),
                (Some(line), false) => line.checked_sub(1),
                (None, true) => Some(self.overlay.top_line + usize::from(!inclusive)),
                (None, false) => self.overlay.top_line.checked_sub(1),
            }
        };
        let found = start.and_then(|from| {
            buf.find_line(from, forward, |line| {
                !matcher.find_matches(&crate::ui::expand_tabs(line)).is_empty()
            })
        });
        match found {
            Some(line) => {
                self.overlay.jump_to_line(line as u64 + 1);
                self.overlay.match_line = Some(line);
                self.overlay.current_line = Some(line);
                if wrapping {
                    self.overlay.message = Some("Wrapped around".to_string());
                }
            }
            None if wrapping => {
                self.overlay.message = Some(format!("Not found: {needle}"));
            }
            None => {
                self.overlay.message = Some("No more matches (press again to wrap)".to_string());
                self.overlay.wrap_armed = Some(direction);
            }
        }
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
        if let Some(flag) = noop_because_it_is_binary {
            match looks_binary(abs) {
                Ok(true) => {
                    *flag = true;
                    return; // binary file, nothing to do
                }
                Ok(false) => {}
                Err(e) => {
                    self.set_status(&format!("Cannot open {}: {}", abs, e), true);
                    return;
                }
            }
        }
        match TextBuffer::open(abs) {
            Ok(buf) => {
                self.show_output = false;
                self.viewer = Some(ViewerContent {
                    title: format!(" {} (Esc to close) ", display_path(abs)),
                    buf,
                    highlight,
                });
                self.overlay.reset_scroll();
                if let Some(line) = jump_line {
                    self.overlay.jump_to_line(line);
                    // Opened on a search hit: that line is the current match, and next/previous
                    // continue from it.
                    if self.viewer.as_ref().is_some_and(|v| v.highlight.is_some()) {
                        let idx = (line as usize).saturating_sub(1);
                        self.overlay.match_line = Some(idx);
                        self.overlay.current_line = Some(idx);
                    }
                }
            }
            Err(e) => self.set_status(&format!("Cannot open {}: {}", abs, e), true),
        }
    }
}

/// The text the full-screen overlay currently shows (file viewer, else command output)
/// with its search highlight. Takes the fields separately so callers can keep using
/// `self.overlay` while holding the result.
pub(super) fn text_view<'a>(
    viewer: &'a mut Option<ViewerContent>,
    show_output: bool,
    last_output: &'a mut Option<TextBuffer>,
    output_highlight: &'a mut Option<(String, bool, bool, bool)>,
) -> Option<(&'a mut TextBuffer, &'a mut Option<(String, bool, bool, bool)>)> {
    if let Some(v) = viewer {
        return Some((&mut v.buf, &mut v.highlight));
    }
    if show_output {
        return last_output.as_mut().map(|b| (b, output_highlight));
    }
    None
}

pub(super) fn shell_escape_path(s: &str) -> String {
    crate::macros::shell_escape(s)
}

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

pub(super) fn is_text_file(abs: &str) -> bool {
    // An unreadable file counts as text, so opening it reports the error.
    !matches!(looks_binary(abs), Ok(true))
}
