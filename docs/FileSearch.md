# File Search

Asynchronous file search: find files by name and optionally by content, starting from the
active panel's directory. The search runs in the background — the UI stays responsive and
results stream in as they are found. Default keys: `Alt-?` or `Ctrl-f` (action `search`).

## Search dialog

Opened by the `search` action. Fields:

- **File pattern** — shell glob by default, regular expression if **RegExp** is checked.
  Empty means all entries (`*`).
- **Containing text** — literal text to look for inside files. Empty means name-only
  search. Matching is a plain substring match honoring **Case sensitive**; regex content
  matching is future work. Files that look binary (NUL byte in the first block) are
  skipped.
- **Max depth** — numeric; empty means unlimited. Depth 1 searches only the root
  directory itself.
- Checkboxes: **RegExp**, **Case sensitive** (applies to both the file pattern and the
  content text), **Include hidden** (pre-seeded from the active panel's current
  hidden-files visibility), **Follow symlinks** (directory symlinks are never followed
  when unchecked; symlinked files are matched by name either way).

OK starts the search rooted at the active panel's current directory.

## Results panel

When the search starts, the active panel is replaced by a *results panel*:

- One entry per matched file or directory. The name column shows the path **relative to
  the search root**; size and mtime columns as usual. For content searches an extra
  column shows the per-file match count.
- Hits appear live in discovery order. When the search completes, the panel's current
  sort order is applied; `Ctrl-s` works as usual afterwards.
- **Enter** behaves like mc: on a directory hit the panel becomes a normal panel showing
  that directory; on a file hit it becomes a normal panel showing the parent directory
  with the file selected.
- **Esc** cancels a running search, closes the results (and matches) panel and restores
  both panels to their previous directories. As everywhere in sc, when the command line
  is not empty the first Esc only enters action mode — so this is effectively Esc Esc.
- Tagging works normally (`Insert`, `*`, `+`, `-`), and quicksearch (`/`) matches over
  the displayed relative paths.
- **F5/F6/F8** operate on the tagged (or selected) hits, with the inactive panel's
  directory as destination — allowed only while the inactive panel is a normal panel
  (i.e. name-only searches); otherwise refused with a status warning.
- Command-line insertion actions keep working: `Alt-Enter` inserts the hit's root-relative
  path (which resolves, because the command line's working directory is the search root),
  `Ctrl-Shift-Enter` the absolute path, `Ctrl-x t` the tagged hits.
- `Alt-r` re-runs the same query. Invoking `search` again from a results panel reopens
  the dialog pre-filled with the current query.

The most recent search a panel jumped away from (via Enter on a hit) stays reachable
through that panel's ordinary back/forward history for the rest of the session:
`Alt-Left`/`Alt-Right` move into and out of it exactly like any other history entry, in
both directions. `Alt-Up` on a search view, live or restored, closes it like Esc-Esc — it
does not additionally navigate to the parent of the search root. History navigation is a
no-op while the matches panel is focused (`Tab` back to the results panel first). Starting
a new search, or `Alt-r`, drops whatever was cached. This isn't persisted to
`panel_history.json` — it's process-memory only, and starting a fresh `sc` session never
resurrects it. If the jump happened before the search reached `Done`, the restored view is
marked `(partial, Alt-r to refresh)` in its footer, since it only ever shows what had been
found up to that point.

## Matches panel (content searches only)

For a content search, the *inactive* panel is replaced by a *matches panel* for the whole
lifetime of the results panel:

- It always shows the matching lines of the file currently selected in the results panel,
  re-syncing as the selection moves. Two columns: **line number** and **text**, with the
  matched substring highlighted (`search_match_fg`/`search_match_bg` color-scheme keys).
- `Tab` switches focus to it as with any panel; Up/Down/PgUp/PgDn scroll the matches.
- **Enter** on a match opens the internal text viewer on that file, jumped to that line.
- While the matches panel is shown, file operations and the command-line/menu actions
  that reference the inactive panel (`%F`, `%D`, `%T`, `Ctrl-x Ctrl-t`, `Ctrl-x Ctrl-p`,
  …) are disabled.

## Text viewer

The output overlay (previously only used for command output in stateless shell mode)
becomes a general full-screen text viewer. In addition to its current role it can display
a file from disk, scroll with the usual keys, open jumped to a given line, and highlight
the search matches. Esc closes it and returns to the search view unchanged.

## Provider API

Search is part of the `TreeProvider` trait (`src/provider/mod.rs`), so future providers
(FTP, archives, …) can implement it natively:

```rust
pub struct LineMatch {
    pub line: u64,        // 1-based
    pub text: String,     // the matching line, no trailing newline
}

pub struct SearchHit {
    pub path: NodePath,          // provider path token of the matched entry
    pub matches: Vec<LineMatch>, // empty for name-only hits
}

pub struct SearchQuery {
    pub pattern: String,          // filename pattern (glob or regex)
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub content: Option<String>,  // literal text; None = name-only search
    pub max_depth: Option<u32>,   // None = unlimited
    pub include_hidden: bool,
    pub follow_symlinks: bool,
}

pub enum SearchEvent {
    Hit(SearchHit),
    Progress { scanning: NodePath, found: usize },
    Done { errors: Vec<String> },
}

pub trait SearchHandle {
    /// Non-blocking. None = no event pending right now.
    fn try_next(&mut self) -> Option<SearchEvent>;
    /// Request the search to stop early.
    fn cancel(&mut self);
}

pub trait TreeProvider {
    // ...existing methods...
    fn search(&self, root: &NodePath, query: SearchQuery) -> Result<Box<dyn SearchHandle>>;
}
```

Contract:

- `search` returns immediately; how the work happens (thread, cooperative chunking,
  remote protocol) is a provider implementation detail. `FilesystemProvider` uses a
  worker thread and an internal channel.
- The UI polls `try_next` once per event-loop tick and drains all pending events.
- The event stream ends with exactly one `Done`, which also follows a `cancel()` request.
  Dropping the handle cancels the search implicitly.
- Unreadable directories/files don't abort the walk; they are collected into
  `Done { errors }`.

`SearchHit` deliberately mirrors external tool output so that planned IPC messages can
feed the same panels: `find -print0` maps to hits with empty `matches`, and
`grep -nZ` output (`filepath\0line:text` records) maps to hits with populated `matches`.
The IPC messages themselves are out of scope for v1.

## Configuration summary

- Keybinding action `search`, default `["Alt-?", "Ctrl-f"]`. The filter dialog moves to
  `Alt-f` to make room.
- Color scheme keys `search_match_fg` / `search_match_bg` — the highlighted match
  substring in the matches panel and in the text viewer.

## Future work

- IPC messages feeding externally produced results (`find -print0`, `grep -nZ`).
- Regex content matching; size/date filters.
- Backgrounded (resumable) search — see @BackgroundedSearch.md for why this was
  considered and deferred rather than built alongside the history caching above.
