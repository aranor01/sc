use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::provider::{LineMatch, NodeEntry, NodePath, SearchQuery};

const MAX_HISTORY: usize = 100;

/// A frozen snapshot of a finished or interrupted search, attached to the
/// history slot of the directory it was rooted in. Replayed (not re-run) when
/// `Alt-Left`/`Alt-Right` land back on that slot.
#[derive(Debug, Clone)]
pub struct CachedSearch {
    pub root: NodePath,
    pub query: SearchQuery,
    pub entries: Vec<NodeEntry>,
    pub matches: HashMap<String, Vec<LineMatch>>,
    /// Root-relative path of the hit that was jumped from, if any, so the
    /// restored view can re-select it.
    pub selected: Option<String>,
    /// Whether the search had already reached `Done` when it was cached, as
    /// opposed to being interrupted mid-scan by the jump away.
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PanelHistory {
    pub entries: Vec<String>,
    /// Parallel to `entries`, index-aligned, moved in lockstep by `push()` so
    /// it can never drift out of sync. Never persisted: a search's cached hit
    /// list is process-memory-only.
    #[serde(skip)]
    pub caches: Vec<Option<Box<CachedSearch>>>,
    #[serde(default)]
    pub index: usize,
}

impl PanelHistory {
    /// The only insertion point. `cache` attaches a replayable search to this
    /// slot; ordinary navigation passes `None`.
    pub fn push_with_cache(&mut self, path: &str, cache: Option<Box<CachedSearch>>) {
        // `caches` is never persisted, so after loading a saved history whose
        // `index` was nonzero, `entries`/`index` come back populated while
        // `caches` deserializes empty. Pad it back into alignment before any
        // index-based operation touches it.
        while self.caches.len() < self.entries.len() {
            self.caches.push(None);
        }
        if self.index > 0 {
            self.entries.drain(0..self.index);
            self.caches.drain(0..self.index);
            self.index = 0;
        }
        self.entries.insert(0, path.to_string());
        self.caches.insert(0, cache);
        self.entries.truncate(MAX_HISTORY);
        self.caches.truncate(MAX_HISTORY);
    }

    pub fn push(&mut self, path: &str) {
        self.push_with_cache(path, None);
    }

    pub fn go_back(&mut self) -> Option<String> {
        if self.index + 1 < self.entries.len() {
            self.index += 1;
            Some(self.entries[self.index].clone())
        } else {
            None
        }
    }

    pub fn go_forward(&mut self) -> Option<String> {
        if self.index > 0 {
            self.index -= 1;
            Some(self.entries[self.index].clone())
        } else {
            None
        }
    }

    pub fn current_path(&self) -> Option<&str> {
        self.entries.get(self.index).map(|s| s.as_str())
    }

    /// The cached search attached to the slot the cursor currently points at,
    /// if any. Checked after `go_back()`/`go_forward()` move the cursor.
    pub fn current_cache(&self) -> Option<&CachedSearch> {
        self.caches.get(self.index).and_then(|c| c.as_deref())
    }

    /// Mutable access to the cached search at the current slot, if any —
    /// used to keep a resident cache in sync with edits (e.g. deletes)
    /// applied to the live panel it was restored into.
    pub fn current_cache_mut(&mut self) -> Option<&mut CachedSearch> {
        self.caches.get_mut(self.index).and_then(|c| c.as_deref_mut())
    }

    /// Drops every cached search for this side without touching `entries` —
    /// directory history itself is unaffected. Called at the start of every
    /// new search, which guarantees at most one `CachedSearch` is ever
    /// resident per side.
    pub fn clear_caches(&mut self) {
        for c in self.caches.iter_mut() {
            *c = None;
        }
    }

    /// Drops the forward part of history (as `push_with_cache` does when
    /// `index > 0`) without inserting a new entry — for callers where the
    /// panel's directory hasn't changed but a new navigation branch has
    /// started (e.g. opening a search view on top of it), so any
    /// previously-reachable forward entries stop being reachable.
    pub fn truncate_forward(&mut self) {
        while self.caches.len() < self.entries.len() {
            self.caches.push(None);
        }
        if self.index > 0 {
            self.entries.drain(0..self.index);
            self.caches.drain(0..self.index);
            self.index = 0;
        }
    }

    pub fn unique_entries(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        self.entries.iter()
            .filter(|e| seen.insert(e.as_str()))
            .map(|e| e.as_str())
            .collect()
    }
}

fn history_path() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("sc")
        .join("panel_history.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HistoryFile {
    left: PanelHistory,
    right: PanelHistory,
}

pub fn load() -> (PanelHistory, PanelHistory) {
    let path = history_path();
    if !path.exists() {
        return Default::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<HistoryFile>(&s).ok())
        .map(|f| (f.left, f.right))
        .unwrap_or_default()
}

pub fn save(left: &PanelHistory, right: &PanelHistory) -> Result<()> {
    let path = history_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = HistoryFile { left: left.clone(), right: right.clone() };
    let json = serde_json::to_string_pretty(&file)?;
    std::fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::NodeKind;

    fn dummy_cache(root: &str) -> Box<CachedSearch> {
        Box::new(CachedSearch {
            root: NodePath(root.to_string()),
            query: SearchQuery {
                pattern: "*".into(),
                is_regex: false,
                case_sensitive: false,
                content: None,
                content_is_regex: false,
                content_case_sensitive: false,
                content_whole_words: false,
                max_depth: None,
                include_hidden: false,
                follow_symlinks: false,
            },
            entries: vec![NodeEntry {
                name: "hit.txt".into(),
                kind: NodeKind::File,
                size: 0,
                modified: std::time::SystemTime::UNIX_EPOCH,
                permissions: String::new(),
            }],
            matches: HashMap::new(),
            selected: Some("hit.txt".into()),
            complete: true,
        })
    }

    #[test]
    fn push_keeps_entries_and_caches_aligned() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push_with_cache("/b", Some(dummy_cache("/b")));
        h.push("/c");
        assert_eq!(h.entries, vec!["/c", "/b", "/a"]);
        assert!(h.caches[0].is_none());
        assert!(h.caches[1].is_some());
        assert!(h.caches[2].is_none());
        assert_eq!(h.caches[1].as_ref().unwrap().root.0, "/b");
    }

    #[test]
    fn go_back_lands_on_cache_via_current_cache() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push_with_cache("/b", Some(dummy_cache("/b")));
        h.push("/x");
        assert!(h.current_cache().is_none());
        let path = h.go_back().unwrap();
        assert_eq!(path, "/b");
        assert!(h.current_cache().is_some());
    }

    #[test]
    fn back_back_forward_forward_is_symmetric() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push_with_cache("/b", Some(dummy_cache("/b")));
        h.push("/x");
        // back: /b (cached), back: /a (plain)
        assert_eq!(h.go_back().unwrap(), "/b");
        assert!(h.current_cache().is_some());
        assert_eq!(h.go_back().unwrap(), "/a");
        assert!(h.current_cache().is_none());
        // forward: /b (cached again), forward: /x (plain)
        assert_eq!(h.go_forward().unwrap(), "/b");
        assert!(h.current_cache().is_some());
        assert_eq!(h.go_forward().unwrap(), "/x");
        assert!(h.current_cache().is_none());
    }

    #[test]
    fn drain_on_push_after_back_keeps_caches_aligned() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push_with_cache("/b", Some(dummy_cache("/b")));
        h.push("/x");
        h.go_back(); // index now points at /b
        h.push("/y"); // should drain the forward entry (/x) from both vectors
        assert_eq!(h.entries, vec!["/y", "/b", "/a"]);
        assert_eq!(h.caches.len(), h.entries.len());
        assert!(h.caches[1].is_some());
    }

    #[test]
    fn truncate_keeps_both_vectors_the_same_length() {
        let mut h = PanelHistory::default();
        for i in 0..(MAX_HISTORY + 10) {
            h.push(&format!("/p{i}"));
        }
        assert_eq!(h.entries.len(), MAX_HISTORY);
        assert_eq!(h.caches.len(), MAX_HISTORY);
    }

    #[test]
    fn clear_caches_drops_caches_but_not_entries() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push_with_cache("/b", Some(dummy_cache("/b")));
        h.push("/x");
        h.clear_caches();
        assert_eq!(h.entries, vec!["/x", "/b", "/a"]);
        assert!(h.caches.iter().all(|c| c.is_none()));
    }

    #[test]
    fn caches_never_serialized() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push_with_cache("/b", Some(dummy_cache("/b")));
        let json = serde_json::to_string(&h).unwrap();
        assert!(!json.contains("CachedSearch"));
        assert!(!json.contains("hit.txt"));
        assert!(json.contains("entries"));
    }

    #[test]
    fn deserializing_plain_history_json_defaults_caches_empty() {
        let json = r#"{"entries":["/a","/b"],"index":0}"#;
        let h: PanelHistory = serde_json::from_str(json).unwrap();
        assert_eq!(h.entries, vec!["/a", "/b"]);
        assert!(h.caches.is_empty());
    }

    #[test]
    fn push_after_deserializing_nonzero_index_does_not_panic() {
        // Mimics a real load(): entries/index restored from disk, caches
        // empty (never persisted).
        let json = r#"{"entries":["/a","/b","/c"],"index":2}"#;
        let mut h: PanelHistory = serde_json::from_str(json).unwrap();
        assert!(h.caches.is_empty());
        h.push("/d"); // used to panic: caches.drain(0..2) on a 0-length vec
        assert_eq!(h.entries, vec!["/d", "/c"]);
        assert_eq!(h.caches.len(), 2);
        assert!(h.caches.iter().all(|c| c.is_none()));
    }

    #[test]
    fn truncate_forward_drops_forward_entries_and_resets_index() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push_with_cache("/b", Some(dummy_cache("/b")));
        h.push("/c");
        h.go_back(); // index now points at /b, /c is a forward entry
        h.truncate_forward();
        assert_eq!(h.entries, vec!["/b", "/a"]);
        assert_eq!(h.index, 0);
        assert_eq!(h.caches.len(), h.entries.len());
        assert!(h.caches[0].is_some()); // /b's cache survives, it wasn't forward
        assert!(h.go_forward().is_none());
    }

    #[test]
    fn truncate_forward_is_a_noop_at_index_zero() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push("/b");
        h.truncate_forward();
        assert_eq!(h.entries, vec!["/b", "/a"]);
        assert_eq!(h.index, 0);
    }

    #[test]
    fn unique_entries_dedupes_repeated_paths() {
        let mut h = PanelHistory::default();
        h.push("/a");
        h.push("/b");
        h.push("/a"); // e.g. the duplicate-D pattern from search-jump caching
        assert_eq!(h.unique_entries(), vec!["/a", "/b"]);
    }
}
