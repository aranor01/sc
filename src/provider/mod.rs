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

pub trait TreeProvider {
    fn root(&self) -> NodePath;
    fn parent(&self, path: &NodePath) -> Option<NodePath>;
    fn join(&self, path: &NodePath, name: &str) -> NodePath;
    fn list(&self, path: &NodePath) -> Result<Vec<NodeEntry>>;
    fn copy(&self, src: &NodePath, dst_dir: &NodePath) -> Result<()>;
    fn move_entry(&self, src: &NodePath, dst_dir: &NodePath) -> Result<()>;
    fn delete(&self, path: &NodePath) -> Result<()>;
}
