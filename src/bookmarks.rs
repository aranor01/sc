use anyhow::Result;
use std::path::PathBuf;

pub fn bookmarks_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("sc")
        .join("bookmarks.json")
}

pub fn load() -> Vec<String> {
    let path = bookmarks_path();
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

pub fn save(bookmarks: &[String]) -> Result<()> {
    let path = bookmarks_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(bookmarks)?;
    std::fs::write(path, json)?;
    Ok(())
}
