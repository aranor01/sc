use super::{NodeEntry, NodeKind, NodePath, Result, TreeProvider};
use anyhow::Context;
use std::path::Path;

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
            std::fs::read_dir(dir).with_context(|| format!("listing {:?}", dir))?
        {
            let raw = raw.with_context(|| format!("reading entry in {:?}", dir))?;
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

    fn rename(&self, path: &NodePath, new_name: &str) -> Result<()> {
        let src = Path::new(&path.0);
        let dst = src
            .parent()
            .ok_or_else(|| anyhow::anyhow!("cannot rename root"))?
            .join(new_name);
        std::fs::rename(src, &dst)
            .with_context(|| format!("renaming {:?} to {:?}", src, dst))
    }
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
