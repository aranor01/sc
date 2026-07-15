pub mod filesystem;

pub type Result<T> = anyhow::Result<T>;

/// Opaque path token whose meaning is defined by each provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodePath(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Dir,
}

#[derive(Debug, Clone)]
pub struct NodeEntry {
    pub name: String,
    pub kind: NodeKind,
    pub size: u64,
    pub modified: std::time::SystemTime,
    pub permissions: String,
}

/// One matching line of a content search (mirrors `grep -nZ` records).
#[derive(Debug, Clone)]
pub struct LineMatch {
    pub line: u64,    // 1-based
    pub text: String, // the matching line, no trailing newline
}

/// One search hit (mirrors `find -print0` when `matches` is empty).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: NodePath,          // provider path token of the matched entry
    pub matches: Vec<LineMatch>, // empty for name-only hits
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub pattern: String,         // filename pattern (glob or regex)
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub content: Option<String>, // literal text; None = name-only search
    pub max_depth: Option<u32>,  // None = unlimited
    pub include_hidden: bool,
    pub follow_symlinks: bool,
}

#[derive(Debug)]
pub enum SearchEvent {
    Hit(SearchHit),
    Progress { scanning: NodePath, found: usize },
    Done { errors: Vec<String> },
}

pub trait SearchHandle {
    /// Non-blocking. None = no event pending right now.
    fn try_next(&mut self) -> Option<SearchEvent>;
    /// Request the search to stop early. A final `Done` still follows.
    fn cancel(&mut self);
}

pub trait TreeProvider {
    fn root(&self) -> NodePath;
    fn parent(&self, path: &NodePath) -> Option<NodePath>;
    fn join(&self, path: &NodePath, name: &str) -> NodePath;
    fn list(&self, path: &NodePath) -> Result<Vec<NodeEntry>>;
    fn copy(&self, src: &NodePath, dst_dir: &NodePath) -> Result<()>;
    fn move_entry(&self, src: &NodePath, dst_dir: &NodePath) -> Result<()>;
    fn delete(&self, path: &NodePath) -> Result<()>;
    fn rename(&self, path: &NodePath, new_name: &str) -> Result<()>;
    fn mkdir(&self, parent: &NodePath, name: &str) -> Result<()>;
    /// Start an asynchronous search rooted at `root`. Returns immediately;
    /// the UI drains the handle once per event-loop tick. The event stream
    /// ends with exactly one `Done` (also after `cancel()`); dropping the
    /// handle cancels the search implicitly.
    fn search(&self, root: &NodePath, query: SearchQuery) -> Result<Box<dyn SearchHandle>>;
}
