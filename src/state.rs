use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::ui::panel::SortKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default = "default_true")]
    pub show_cmdline: bool,
    #[serde(default = "default_true")]
    pub show_button_bar: bool,
    #[serde(default)]
    pub left_sort_key: SortKey,
    #[serde(default = "default_true")]
    pub left_sort_asc: bool,
    #[serde(default)]
    pub right_sort_key: SortKey,
    #[serde(default = "default_true")]
    pub right_sort_asc: bool,
    #[serde(default)]
    pub left_show_hidden: bool,
    #[serde(default)]
    pub right_show_hidden: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            orientation: Orientation::Vertical,
            show_cmdline: true,
            show_button_bar: true,
            left_sort_key: SortKey::Name,
            left_sort_asc: true,
            right_sort_key: SortKey::Name,
            right_sort_asc: true,
            left_show_hidden: false,
            right_show_hidden: false,
        }
    }
}

fn state_path() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("sc")
        .join("state.json")
}

pub fn history_path() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("sc")
        .join("command_history")
}

impl AppState {
    pub fn load() -> Self {
        let path = state_path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = state_path();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}
