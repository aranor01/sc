//! Text shown by the full-screen viewer: raw bytes read lazily in chunks, with a
//! lazily extended index of line starts. A line is only decoded and sanitized
//! when it is asked for, so opening a file costs one chunk read whatever its
//! size, and only the lines on screen (or measured for wrapping) are ever cleaned.

use std::borrow::Cow;
use std::fs::File;
use std::io::Read;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ui::is_format_char;

const CHUNK: usize = 1 << 20;
/// Window checked for NUL bytes by `looks_binary`.
const BINARY_PROBE: usize = 8192;

// OSC alternative is first so \x1b] is consumed with its body;
// otherwise the fallback 2-char match would consume just \x1b].
// [0-?] adds DEC private sequences (\x1b=, \x1b>, \x1b7, \x1b8, etc.) missing from the
// original [@-Z\\-_] range (0x40-0x5F).
pub static ANSI_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\x1b(?:\][^\x07\x1b]*(?:\x07|\x1b\\)|[0-?@-Z\\-_]|\[[0-?]*[ -/]*[@-~])",
    )
    .unwrap()
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct TextBuffer {
    /// Distinguishes buffers, so a view cache built for one can be dropped when
    /// another is shown.
    id: u64,
    /// `None` once the whole file has been read, and for in-memory text.
    file: Option<File>,
    chunk: usize,
    /// Raw bytes read so far.
    data: Vec<u8>,
    /// `starts[i]` = byte offset of line `i`, for every line whose start is known.
    starts: Vec<usize>,
    /// `data[..scanned]` has been searched for `\n`.
    scanned: usize,
    eof: bool,
    /// Size of the file when it was opened; only used to estimate the line count.
    file_len: u64,
}

impl TextBuffer {
    /// Opens `path`, reading its first chunk only.
    pub fn open(path: &str) -> std::io::Result<Self> {
        Self::open_with_chunk(path, CHUNK)
    }

    pub(crate) fn open_with_chunk(path: &str, chunk: usize) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut buf = Self::new(Some(file), Vec::new(), file_len, chunk);
        buf.fill();
        Ok(buf)
    }

    /// Text that is already in memory, e.g. a command's output.
    pub fn from_string(text: String) -> Self {
        let len = text.len() as u64;
        let mut buf = Self::new(None, text.into_bytes(), len, CHUNK);
        buf.scan();
        buf.finish();
        buf
    }

    fn new(file: Option<File>, data: Vec<u8>, file_len: u64, chunk: usize) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            file,
            chunk,
            data,
            starts: vec![0],
            scanned: 0,
            eof: false,
            file_len,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Records the start of every line found in the bytes not yet scanned.
    fn scan(&mut self) {
        let from = self.scanned;
        self.starts
            .extend(memchr::memchr_iter(b'\n', &self.data[from..]).map(|p| from + p + 1));
        self.scanned = self.data.len();
    }

    /// Marks everything as read. A start at the very end of the data is not a line
    /// (the text ends with a newline), matching `str::lines`.
    fn finish(&mut self) {
        self.eof = true;
        self.file = None;
        if self.starts.last() == Some(&self.data.len()) {
            self.starts.pop();
        }
    }

    /// Reads one more chunk and indexes it. A read error counts as end of file.
    fn fill(&mut self) {
        let Some(file) = self.file.as_mut() else {
            self.finish();
            return;
        };
        let read = file.by_ref().take(self.chunk as u64).read_to_end(&mut self.data);
        self.scan();
        if read.map_or(true, |n| n < self.chunk) {
            self.finish();
        }
    }

    /// Reads as far as needed to know whether line `idx` exists; true if it does.
    pub fn ensure_line(&mut self, idx: usize) -> bool {
        // Line `idx` ends at the start of line `idx + 1`, so that must be known too.
        while self.starts.len() <= idx + 1 && !self.eof {
            self.fill();
        }
        idx < self.starts.len()
    }

    /// Line `idx`, sanitized for display, if it has been read (see `ensure_line`).
    pub fn line(&self, idx: usize) -> Option<Cow<'_, str>> {
        let start = *self.starts.get(idx)?;
        let end = match self.starts.get(idx + 1) {
            Some(&next) => next,
            None if self.eof => self.data.len(),
            None => return None, // end not known yet
        };
        let raw = &self.data[start..end];
        let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
        Some(sanitize_line(raw))
    }

    /// Number of lines whose extent is known so far: a lower bound until `eof`.
    pub fn known_lines(&self) -> usize {
        if self.eof {
            self.starts.len()
        } else {
            self.starts.len().saturating_sub(1)
        }
    }

    /// Best guess of the total line count, for the scrollbar. Exact at end of file.
    pub fn estimated_lines(&self) -> usize {
        let known = self.known_lines();
        if self.eof || self.scanned == 0 {
            return known;
        }
        let estimate = (known as u128 * self.file_len as u128 / self.scanned as u128) as usize;
        estimate.max(known)
    }

    #[cfg(test)]
    pub fn fully_read(&self) -> bool {
        self.eof
    }

    /// Every line, joined with `\n`. Reads the whole file.
    #[cfg(test)]
    pub fn full_text(&mut self) -> String {
        let mut out = Vec::new();
        let mut idx = 0;
        while self.ensure_line(idx) {
            out.push(self.line(idx).unwrap().into_owned());
            idx += 1;
        }
        out.join("\n")
    }
}

/// Whether `path` looks like a binary file: a NUL byte in its first block.
pub fn looks_binary(path: &str) -> std::io::Result<bool> {
    let mut head = Vec::with_capacity(BINARY_PROBE);
    File::open(path)?.take(BINARY_PROBE as u64).read_to_end(&mut head)?;
    Ok(head.contains(&0u8))
}

/// Whether `c` has to be dropped or replaced when showing a file.
fn needs_sanitizing(c: char) -> bool {
    match c {
        '\n' | '\t' => false,
        c if c.is_control() => true,
        c => is_format_char(c),
    }
}

/// Cleans one line (no line terminator) of file content for display: invalid UTF-8
/// becomes U+FFFD, `\r` is dropped, ANSI escape sequences and other control
/// characters become `.`, and format characters (bidi overrides, zero-width
/// joiners) are dropped. Tabs are kept. A clean line is borrowed, not copied.
pub fn sanitize_line(raw: &[u8]) -> Cow<'_, str> {
    let s = String::from_utf8_lossy(raw);
    let clean = if s.is_ascii() {
        !s.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7f)
    } else {
        !s.chars().any(needs_sanitizing)
    };
    if clean {
        return s;
    }
    let mut s = s.replace('\r', "");
    if s.contains('\x1b') {
        s = ANSI_RE
            .replace_all(&s, |caps: &regex::Captures| {
                ".".repeat(caps.get(0).unwrap().as_str().chars().count())
            })
            .into_owned();
    }
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' => out.push(ch),
            c if c.is_control() => out.push('.'),
            c if is_format_char(c) => {}
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Reads `text` back through a file split into `chunk`-byte reads.
    fn lines_via_file(text: &[u8], chunk: usize) -> Vec<String> {
        let dir = tempfile_dir();
        let path = dir.join("f.txt");
        File::create(&path).unwrap().write_all(text).unwrap();
        let mut buf = TextBuffer::open_with_chunk(path.to_str().unwrap(), chunk).unwrap();
        let mut out = Vec::new();
        let mut idx = 0;
        while buf.ensure_line(idx) {
            out.push(buf.line(idx).unwrap().into_owned());
            idx += 1;
        }
        std::fs::remove_dir_all(dir).unwrap();
        out
    }

    fn tempfile_dir() -> std::path::PathBuf {
        use std::sync::atomic::AtomicUsize;
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sc-textbuf-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lines_match_str_lines_for_every_chunk_size() {
        let cases = [
            "", "\n", "a", "a\n", "a\nb", "a\n\nb\n", "\n\na", "a\nb\nc\n", "héllo\nwörld ✓\n",
            "abc\r\ndef\r\n", "no newline at all, a fairly long single line",
        ];
        for text in cases {
            let want: Vec<String> = text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
            for chunk in [1, 2, 3, 4, 5, 7, 64, CHUNK] {
                assert_eq!(lines_via_file(text.as_bytes(), chunk), want, "text {text:?} chunk {chunk}");
            }
            let mut mem = TextBuffer::from_string(text.to_string());
            assert_eq!(mem.full_text(), want.join("\n"), "in-memory {text:?}");
        }
    }

    #[test]
    fn open_reads_only_the_first_chunk() {
        let dir = tempfile_dir();
        let path = dir.join("big.txt");
        File::create(&path).unwrap().write_all("line\n".repeat(100).as_bytes()).unwrap();
        let mut buf = TextBuffer::open_with_chunk(path.to_str().unwrap(), 50).unwrap();
        assert!(!buf.eof);
        assert_eq!(buf.data.len(), 50);
        assert!(buf.ensure_line(3));
        assert!(!buf.eof, "reading a few lines must not read the rest of the file");
        assert!(buf.data.len() < 500);
        assert_eq!(buf.known_lines(), 10, "the line starting at the chunk edge has no known end yet");
        assert!(buf.estimated_lines() >= 90, "estimate scales with file size");
        assert!(buf.ensure_line(99));
        assert!(!buf.ensure_line(100));
        assert!(buf.eof);
        assert_eq!(buf.estimated_lines(), 100);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_utf8_and_multibyte_split_across_chunks() {
        assert_eq!(lines_via_file(b"ok\xffok\n", 3), ["ok\u{fffd}ok"]);
        assert_eq!(lines_via_file("a✓b\n".as_bytes(), 2), ["a✓b"]);
    }

    #[test]
    fn looks_binary_checks_for_nul_in_first_block() {
        let dir = tempfile_dir();
        let text = dir.join("t");
        let bin = dir.join("b");
        std::fs::write(&text, "plain\n").unwrap();
        std::fs::write(&bin, b"ab\0cd").unwrap();
        assert!(!looks_binary(text.to_str().unwrap()).unwrap());
        assert!(looks_binary(bin.to_str().unwrap()).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sanitize_line_borrows_clean_text() {
        assert!(matches!(sanitize_line("a\tb ünï 日本".as_bytes()), Cow::Borrowed(_)));
    }

    #[test]
    fn sanitize_line_cleans_cr_ansi_controls_and_format_chars() {
        assert_eq!(sanitize_line(b"a\r"), "a");
        assert_eq!(sanitize_line(b"x\x1b[31mred\x1b[0m"), "x.....red....");
        assert_eq!(sanitize_line(b"a\x01b\x7fc\x1b"), "a.b.c.");
        assert_eq!(sanitize_line("a\u{202e}b".as_bytes()), "ab");
        assert_eq!(sanitize_line(b"ok\xffok"), "ok\u{fffd}ok");
    }
}
