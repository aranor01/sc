use super::{
    LineMatch, NodeEntry, NodeKind, NodePath, Result, SearchEvent, SearchHandle, SearchHit,
    SearchQuery, TreeProvider,
};
use crate::pattern::{build_filter_pattern, ContentMatcher, FilterPattern};
use anyhow::Context;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub struct FilesystemProvider;

impl TreeProvider for FilesystemProvider {
    fn root(&self) -> NodePath {
        NodePath("/".to_string())
    }

    fn parent(&self, path: &NodePath) -> Option<NodePath> {
        Path::new(&path.0)
            .parent()
            .map(|p| NodePath(p.to_string_lossy().into_owned()))
    }

    fn join(&self, path: &NodePath, name: &str) -> NodePath {
        NodePath(Path::new(&path.0).join(name).to_string_lossy().into_owned())
    }

    fn list(&self, path: &NodePath) -> Result<Vec<NodeEntry>> {
        let dir = Path::new(&path.0);
        let mut entries = Vec::new();

        for raw in
            std::fs::read_dir(dir).with_context(|| format!("Cannot list {:?}", dir))?
        {
            let raw = raw.with_context(|| format!("Cannot read entry in {:?}", dir))?;
            let entry_path = raw.path();
            let name = raw.file_name().to_string_lossy().into_owned();

            // lstat — used for the permissions string (shows 'l' for symlinks).
            let sym_meta = raw
                .metadata()
                .with_context(|| format!("lstat {:?}", entry_path))?;

            // stat — follows symlinks for kind/size; falls back if target is broken.
            let meta = std::fs::metadata(&entry_path).unwrap_or_else(|_| sym_meta.clone());

            entries.push(NodeEntry {
                name,
                kind: if meta.is_dir() { NodeKind::Dir } else { NodeKind::File },
                size: meta.len(),
                modified: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                permissions: unix_permissions(&sym_meta),
            });
        }

        // Dirs first; within each group, case-insensitive lexicographic order.
        entries.sort_by(|a, b| {
            use std::cmp::Ordering;
            match (&a.kind, &b.kind) {
                (NodeKind::Dir, NodeKind::File) => Ordering::Less,
                (NodeKind::File, NodeKind::Dir) => Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        Ok(entries)
    }

    fn copy(&self, src: &NodePath, dst_dir: &NodePath) -> Result<()> {
        let src_path = Path::new(&src.0);
        let filename = src_path
            .file_name()
            .with_context(|| format!("source {:?} has no filename component", src_path))?;
        copy_recursive(src_path, &Path::new(&dst_dir.0).join(filename))
    }

    fn move_entry(&self, src: &NodePath, dst_dir: &NodePath) -> Result<()> {
        let src_path = Path::new(&src.0);
        let filename = src_path
            .file_name()
            .with_context(|| format!("source {:?} has no filename component", src_path))?;
        let dst_path = Path::new(&dst_dir.0).join(filename);

        // Prefer atomic rename (works within the same filesystem).
        if std::fs::rename(src_path, &dst_path).is_ok() {
            return Ok(());
        }

        // Cross-filesystem fallback: copy then delete source.
        copy_recursive(src_path, &dst_path)
            .with_context(|| format!("copying {:?} to {:?}", src_path, dst_path))?;
        if src_path.is_dir() {
            std::fs::remove_dir_all(src_path)
        } else {
            std::fs::remove_file(src_path)
        }
        .with_context(|| format!("removing source {:?} after cross-device move", src_path))
    }

    fn delete(&self, path: &NodePath) -> Result<()> {
        let p = Path::new(&path.0);
        if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        }
        .with_context(|| format!("deleting {:?}", p))
    }

    fn mkdir(&self, parent: &NodePath, name: &str) -> Result<()> {
        let path = Path::new(&parent.0).join(name);
        std::fs::create_dir(&path)
            .with_context(|| format!("creating directory {:?}", path))
    }

    fn rename(&self, path: &NodePath, new_name: &str) -> Result<()> {
        let src = Path::new(&path.0);
        let dst = src
            .parent()
            .ok_or_else(|| anyhow::anyhow!("cannot rename root"))?
            .join(new_name);
        std::fs::rename(src, &dst)
            .with_context(|| format!("renaming {:?} to {:?}", src, dst))
    }

    fn search(&self, root: &NodePath, query: SearchQuery) -> Result<Box<dyn SearchHandle>> {
        let pattern = build_filter_pattern(
            &query.pattern,
            false,
            query.case_sensitive,
            query.is_regex,
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        let content_matcher = match &query.content {
            Some(needle) => Some(
                ContentMatcher::build(
                    needle,
                    query.content_is_regex,
                    query.content_case_sensitive,
                    query.content_whole_words,
                )
                .map_err(|e| anyhow::anyhow!(e))?,
            ),
            None => None,
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let root = PathBuf::from(&root.0);
        std::thread::spawn(move || search_worker(root, query, pattern, content_matcher, tx, flag));
        Ok(Box::new(FsSearchHandle { rx, cancel }))
    }
}

struct FsSearchHandle {
    rx: mpsc::Receiver<SearchEvent>,
    cancel: Arc<AtomicBool>,
}

impl SearchHandle for FsSearchHandle {
    fn try_next(&mut self) -> Option<SearchEvent> {
        self.rx.try_recv().ok()
    }

    fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for FsSearchHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Longest stored matching line; anything longer is cut down to a window
/// centered on the first match (at a char boundary) so a minified file can't
/// blow up memory, without losing the match itself off the truncated end.
const MAX_MATCH_LINE: usize = 1000;

/// Keeps `text` within `MAX_MATCH_LINE` bytes, centering the kept window on
/// `first_hit` (a byte range within `text`) instead of always keeping the
/// start, so a match far into a long line survives the cap.
fn cap_match_line(text: &str, first_hit: (usize, usize)) -> String {
    if text.len() <= MAX_MATCH_LINE {
        return text.to_string();
    }
    let (h_start, h_end) = first_hit;
    // Cap the span considered for centering at MAX_MATCH_LINE: for a match
    // longer than that (e.g. a greedy regex spanning most of the line), the
    // uncapped midpoint could land the window entirely past `h_start`,
    // excluding the match's own beginning. This is a no-op when the match
    // already fits.
    let span = h_end.saturating_sub(h_start).min(MAX_MATCH_LINE);
    let center = h_start + span / 2;
    let half = MAX_MATCH_LINE / 2;
    let mut start = center.saturating_sub(half);
    if start + MAX_MATCH_LINE > text.len() {
        start = text.len().saturating_sub(MAX_MATCH_LINE);
    }
    let mut end = (start + MAX_MATCH_LINE).min(text.len());
    // Snap inward (never outward) so the window never grows past MAX_MATCH_LINE.
    while start < end && !text.is_char_boundary(start) {
        start += 1;
    }
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[start..end].to_string()
}

fn search_worker(
    root: PathBuf,
    query: SearchQuery,
    pattern: FilterPattern,
    content_matcher: Option<ContentMatcher>,
    tx: mpsc::Sender<SearchEvent>,
    cancel: Arc<AtomicBool>,
) {
    let mut errors: Vec<String> = Vec::new();
    let mut found = 0usize;
    // Only symlinked directories can introduce cycles (a plain walk without
    // symlinks is a tree). Seeded with the root so a symlink pointing back
    // at it is caught too.
    let mut visited_dirs: std::collections::HashSet<PathBuf> = std::fs::canonicalize(&root)
        .into_iter()
        .collect();
    let mut stack: Vec<(PathBuf, u32)> = vec![(root, 1)];

    'walk: while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let progress = SearchEvent::Progress {
            scanning: NodePath(dir.to_string_lossy().into_owned()),
            found,
        };
        if tx.send(progress).is_err() {
            return; // receiver gone: handle dropped, stop silently
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                errors.push(format!("{}: {}", dir.display(), e));
                continue;
            }
        };
        for raw in rd {
            if cancel.load(Ordering::Relaxed) {
                break 'walk;
            }
            let raw = match raw {
                Ok(raw) => raw,
                Err(e) => {
                    errors.push(format!("{}: {}", dir.display(), e));
                    continue;
                }
            };
            let name = raw.file_name().to_string_lossy().into_owned();
            if !query.include_hidden && name.starts_with('.') {
                continue;
            }
            let path = raw.path();
            // DirEntry::metadata does not follow symlinks (lstat).
            let is_symlink = raw
                .metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            let follow_meta = if is_symlink {
                std::fs::metadata(&path).ok()
            } else {
                raw.metadata().ok()
            };
            let is_dir = follow_meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let is_regular_file = follow_meta.as_ref().map(|m| m.is_file()).unwrap_or(false);

            if is_dir
                && (!is_symlink || query.follow_symlinks)
                && query.max_depth.map(|d| depth < d).unwrap_or(true)
            {
                if is_symlink {
                    // Only descend if we haven't already visited this
                    // directory by its canonical (cycle-resolved) identity.
                    if let Ok(canon) = std::fs::canonicalize(&path) {
                        if visited_dirs.insert(canon) {
                            stack.push((path.clone(), depth + 1));
                        }
                    }
                } else {
                    stack.push((path.clone(), depth + 1));
                }
            }

            if !pattern.matches(&name) {
                continue;
            }
            let hit = match &content_matcher {
                None => Some(Vec::new()),
                Some(_) if is_dir => None,
                // FIFOs, sockets and devices can block or misbehave on open;
                // only regular files are content-scanned. Name-only hits are
                // unaffected — this only skips the content-match attempt.
                Some(_) if !is_regular_file => None,
                Some(matcher) => match scan_file(&path, matcher) {
                    Ok(Some(matches)) if !matches.is_empty() => Some(matches),
                    Ok(_) => None, // binary file or no matching lines
                    Err(e) => {
                        errors.push(format!("{}: {}", path.display(), e));
                        None
                    }
                },
            };
            if let Some(matches) = hit {
                found += 1;
                let hit = SearchHit {
                    path: NodePath(path.to_string_lossy().into_owned()),
                    matches,
                };
                if tx.send(SearchEvent::Hit(hit)).is_err() {
                    return;
                }
            }
        }
    }
    let _ = tx.send(SearchEvent::Done { errors });
}

/// Scan a file for lines containing `needle`. Returns `Ok(None)` for files
/// that look binary (NUL byte in the first buffered block).
fn scan_file(
    path: &Path,
    matcher: &ContentMatcher,
) -> std::io::Result<Option<Vec<LineMatch>>> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    if reader.fill_buf()?.contains(&0) {
        return Ok(None);
    }
    let mut matches = Vec::new();
    let mut line_no = 0u64;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        line_no += 1;
        while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
            buf.pop();
        }
        let text = String::from_utf8_lossy(&buf);
        let hits = matcher.find_matches(&text);
        if let Some(&first_hit) = hits.first() {
            let text = text.into_owned();
            let text = cap_match_line(&text, first_hit);
            matches.push(LineMatch { line: line_no, text });
        }
    }
    Ok(Some(matches))
}

fn copy_recursive(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)
            .with_context(|| format!("creating directory {:?}", dst))?;
        for entry in
            std::fs::read_dir(src).with_context(|| format!("reading {:?}", src))?
        {
            let entry = entry.with_context(|| format!("reading entry in {:?}", src))?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(src, dst)
            .with_context(|| format!("copying {:?} to {:?}", src, dst))?;
    }
    Ok(())
}

fn unix_permissions(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let ft = meta.file_type();
    let type_char = if ft.is_dir() {
        'd'
    } else if ft.is_symlink() {
        'l'
    } else {
        '-'
    };
    let mut s = String::with_capacity(10);
    s.push(type_char);
    for &(bit, ch) in &[
        (0o400u32, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040,    'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004,    'r'), (0o002, 'w'), (0o001, 'x'),
    ] {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn query(pattern: &str) -> SearchQuery {
        SearchQuery {
            pattern: pattern.to_string(),
            is_regex: false,
            case_sensitive: true,
            content: None,
            content_is_regex: false,
            content_case_sensitive: true,
            content_whole_words: false,
            max_depth: None,
            include_hidden: false,
            follow_symlinks: false,
        }
    }

    fn make_base(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("sc_search_test_{name}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn start(root: &Path, q: SearchQuery) -> Box<dyn SearchHandle> {
        FilesystemProvider
            .search(&NodePath(root.to_string_lossy().into_owned()), q)
            .unwrap()
    }

    fn collect(handle: &mut dyn SearchHandle) -> (Vec<SearchHit>, Vec<String>) {
        let mut hits = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match handle.try_next() {
                Some(SearchEvent::Hit(h)) => hits.push(h),
                Some(SearchEvent::Progress { .. }) => {}
                Some(SearchEvent::Done { errors }) => return (hits, errors),
                None => {
                    assert!(Instant::now() < deadline, "search did not finish in time");
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }

    fn run(root: &Path, q: SearchQuery) -> (Vec<SearchHit>, Vec<String>) {
        let mut handle = start(root, q);
        let out = collect(&mut *handle);
        assert!(handle.try_next().is_none(), "events after Done");
        out
    }

    fn rel_names(root: &Path, hits: &[SearchHit]) -> Vec<String> {
        let mut v: Vec<String> = hits
            .iter()
            .map(|h| {
                Path::new(&h.path.0)
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn name_search_glob_recurses() {
        let base = make_base("glob");
        std::fs::write(base.join("a.rs"), "x").unwrap();
        std::fs::write(base.join("b.txt"), "x").unwrap();
        std::fs::create_dir(base.join("sub")).unwrap();
        std::fs::write(base.join("sub").join("c.rs"), "x").unwrap();

        let (hits, errors) = run(&base, query("*.rs"));
        assert!(errors.is_empty());
        assert_eq!(rel_names(&base, &hits), vec!["a.rs", "sub/c.rs"]);
        assert!(hits.iter().all(|h| h.matches.is_empty()));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn name_search_matches_directories_too() {
        let base = make_base("dirs");
        std::fs::create_dir(base.join("target")).unwrap();
        std::fs::write(base.join("target.txt"), "x").unwrap();

        let (hits, _) = run(&base, query("target*"));
        assert_eq!(rel_names(&base, &hits), vec!["target", "target.txt"]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn regex_name_search_case_insensitive() {
        let base = make_base("regex");
        std::fs::write(base.join("Main.rs"), "x").unwrap();
        std::fs::write(base.join("other.rs"), "x").unwrap();

        let mut q = query("^ma");
        q.is_regex = true;
        q.case_sensitive = false;
        let (hits, _) = run(&base, q);
        assert_eq!(rel_names(&base, &hits), vec!["Main.rs"]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hidden_entries_skipped_unless_included() {
        let base = make_base("hidden");
        std::fs::write(base.join(".hidden.rs"), "x").unwrap();
        std::fs::create_dir(base.join(".dir")).unwrap();
        std::fs::write(base.join(".dir").join("inner.rs"), "x").unwrap();
        std::fs::write(base.join("plain.rs"), "x").unwrap();

        let (hits, _) = run(&base, query("*.rs"));
        assert_eq!(rel_names(&base, &hits), vec!["plain.rs"]);

        let mut q = query("*.rs");
        q.include_hidden = true;
        let (hits, _) = run(&base, q);
        assert_eq!(
            rel_names(&base, &hits),
            vec![".dir/inner.rs", ".hidden.rs", "plain.rs"]
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn max_depth_limits_recursion() {
        let base = make_base("depth");
        std::fs::write(base.join("top.rs"), "x").unwrap();
        std::fs::create_dir(base.join("sub")).unwrap();
        std::fs::write(base.join("sub").join("deep.rs"), "x").unwrap();

        let mut q = query("*.rs");
        q.max_depth = Some(1);
        let (hits, _) = run(&base, q);
        assert_eq!(rel_names(&base, &hits), vec!["top.rs"]);

        let mut q = query("*.rs");
        q.max_depth = Some(2);
        let (hits, _) = run(&base, q);
        assert_eq!(rel_names(&base, &hits), vec!["sub/deep.rs", "top.rs"]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn content_search_reports_lines_and_skips_binaries_and_dirs() {
        let base = make_base("content");
        std::fs::write(base.join("hit.txt"), "nope\nfound HERE\nagain here\n").unwrap();
        std::fs::write(base.join("miss.txt"), "nothing\n").unwrap();
        std::fs::write(base.join("bin.txt"), b"here\0here").unwrap();
        std::fs::create_dir(base.join("here_dir")).unwrap();

        let mut q = query("*");
        q.content = Some("here".to_string());
        q.content_case_sensitive = false;
        let (hits, errors) = run(&base, q);
        assert!(errors.is_empty());
        assert_eq!(rel_names(&base, &hits), vec!["hit.txt"]);
        let m = &hits[0].matches;
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].line, m[0].text.as_str()), (2, "found HERE"));
        assert_eq!((m[1].line, m[1].text.as_str()), (3, "again here"));

        // Case-sensitive: only the lowercase line matches.
        let mut q = query("*");
        q.content = Some("here".to_string());
        let (hits, _) = run(&base, q);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].matches.len(), 1);
        assert_eq!(hits[0].matches[0].line, 3);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn content_search_regex_matches() {
        let base = make_base("content_regex");
        std::fs::write(base.join("hit.txt"), "cat123\nno digits here\n").unwrap();

        let mut q = query("*");
        q.content = Some(r"cat\d+".to_string());
        q.content_is_regex = true;
        let (hits, errors) = run(&base, q);
        assert!(errors.is_empty());
        assert_eq!(rel_names(&base, &hits), vec!["hit.txt"]);
        assert_eq!(hits[0].matches.len(), 1);
        assert_eq!(hits[0].matches[0].line, 1);

        // A literal-mode search for the same text finds nothing, confirming
        // content_is_regex actually toggles matching mode.
        let mut q = query("*");
        q.content = Some(r"cat\d+".to_string());
        let (hits, _) = run(&base, q);
        assert!(hits.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn content_search_whole_words() {
        let base = make_base("content_whole_words");
        std::fs::write(base.join("hit.txt"), "a cat sat\n").unwrap();
        std::fs::write(base.join("miss.txt"), "category error\n").unwrap();

        let mut q = query("*");
        q.content = Some("cat".to_string());
        q.content_whole_words = true;
        let (hits, errors) = run(&base, q);
        assert!(errors.is_empty());
        assert_eq!(rel_names(&base, &hits), vec!["hit.txt"]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn content_search_skips_fifo_instead_of_blocking() {
        let base = make_base("fifo");
        let fifo_path = base.join("pipe.rs");
        let c_path = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
        let ret = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(ret, 0, "mkfifo failed: {}", std::io::Error::last_os_error());
        std::fs::write(base.join("real.rs"), "needle here\n").unwrap();

        let mut q = query("*.rs");
        q.content = Some("needle".to_string());
        // run()'s collect() has a 10s deadline: opening the FIFO (no writer
        // ever attaches) used to block the worker thread forever, so a
        // regression here fails the assertion instead of hanging.
        let (hits, errors) = run(&base, q);
        assert!(errors.is_empty());
        assert_eq!(rel_names(&base, &hits), vec!["real.rs"]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn content_search_keeps_far_match_visible_on_long_line() {
        let base = make_base("long_line_match");
        let line = format!("{}needle{}", "A".repeat(2000), "B".repeat(2000));
        std::fs::write(base.join("hit.txt"), format!("{line}\n")).unwrap();

        let mut q = query("*");
        q.content = Some("needle".to_string());
        let (hits, errors) = run(&base, q);
        assert!(errors.is_empty());
        assert_eq!(rel_names(&base, &hits), vec!["hit.txt"]);
        assert_eq!(hits[0].matches.len(), 1);
        let m = &hits[0].matches[0];
        assert_eq!(m.line, 1);
        assert!(m.text.len() <= MAX_MATCH_LINE);
        assert!(m.text.contains("needle"), "match must survive the length cap: {:?}", m.text);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cap_match_line_never_exceeds_the_cap_on_multi_byte_boundaries() {
        // Every char is 3 bytes ('€'), so a computed window edge lands mid-char
        // unless it happens to be a multiple of 3 — chosen here so BOTH the
        // start and end land mid-character, which used to grow the window
        // (by 2 bytes) past MAX_MATCH_LINE instead of shrinking it.
        let text = "\u{20ac}".repeat(2000);
        let result = cap_match_line(&text, (3000, 3000));
        assert!(
            result.len() <= MAX_MATCH_LINE,
            "window must never exceed the cap: got {} bytes",
            result.len()
        );
    }

    #[test]
    fn cap_match_line_keeps_the_matchs_start_when_the_match_spans_more_than_the_cap() {
        // A match spanning the whole 3000-byte line (e.g. a greedy `A.*Z`
        // regex) is wider than MAX_MATCH_LINE; centering on its raw midpoint
        // would land the kept window entirely past byte 0, dropping the "A"
        // that made it match in the first place.
        let text = format!("A{}Z", "x".repeat(2998));
        let result = cap_match_line(&text, (0, text.len()));
        assert!(result.len() <= MAX_MATCH_LINE);
        assert!(result.starts_with('A'), "window must include the match's own start: {:?}", result);
    }

    #[test]
    fn symlinked_dirs_followed_only_on_request() {
        let base = make_base("symlink");
        std::fs::create_dir(base.join("real")).unwrap();
        std::fs::write(base.join("real").join("inside.rs"), "x").unwrap();
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();

        let (hits, _) = run(&base, query("inside.rs"));
        assert_eq!(rel_names(&base, &hits), vec!["real/inside.rs"]);

        let mut q = query("inside.rs");
        q.follow_symlinks = true;
        let (hits, _) = run(&base, q);
        assert_eq!(
            rel_names(&base, &hits),
            vec!["link/inside.rs", "real/inside.rs"]
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn symlink_cycle_does_not_loop_forever() {
        let base = make_base("cycle");
        std::fs::create_dir(base.join("a")).unwrap();
        std::fs::write(base.join("a").join("marker.rs"), "x").unwrap();
        // a/loop -> base: a cycle back to an ancestor already on the walk.
        std::os::unix::fs::symlink(&base, base.join("a").join("loop")).unwrap();

        let mut q = query("marker.rs");
        q.follow_symlinks = true;
        let (hits, _) = run(&base, q); // run()'s collect() has a 10s deadline,
        // so a regression here fails the assertion rather than hanging.
        assert_eq!(rel_names(&base, &hits), vec!["a/marker.rs"]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn symlinked_file_matched_by_name_either_way() {
        let base = make_base("symfile");
        std::fs::write(base.join("real.rs"), "x").unwrap();
        std::os::unix::fs::symlink(base.join("real.rs"), base.join("alias.rs")).unwrap();

        let (hits, _) = run(&base, query("alias.rs"));
        assert_eq!(rel_names(&base, &hits), vec!["alias.rs"]);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cancel_still_delivers_done() {
        let base = make_base("cancel");
        for i in 0..50 {
            let d = base.join(format!("d{i}"));
            std::fs::create_dir(&d).unwrap();
            std::fs::write(d.join("f.rs"), "x").unwrap();
        }
        let mut handle = start(&base, query("*.rs"));
        handle.cancel();
        let (_, _) = collect(&mut *handle); // must terminate with Done
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn unreadable_dir_collected_into_errors() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignores permissions; nothing to test
        }
        let base = make_base("unreadable");
        let locked = base.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("secret.rs"), "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let (hits, errors) = run(&base, query("*.rs"));
        assert!(hits.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("locked"));

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
