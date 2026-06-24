use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_HISTORY: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PanelHistory {
    pub entries: Vec<String>,
}

impl PanelHistory {
    pub fn push(&mut self, path: &str) {
        // Remove consecutive duplicates
        if self.entries.first().map(String::as_str) == Some(path) {
            return;
        }
        self.entries.insert(0, path.to_string());
        self.entries.truncate(MAX_HISTORY);
    }
}

fn history_path() -> PathBuf {
    dirs::data_local_dir()
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
