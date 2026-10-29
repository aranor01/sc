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

/// Settings of the internal text viewer, saved under `"viewer"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerState {
    #[serde(default = "default_true")]
    pub show_line_numbers: bool,
}

impl Default for ViewerState {
    fn default() -> Self {
        ViewerState { show_line_numbers: true }
    }
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
    /// Most recent command-palette picks, newest first (palette ids).
    #[serde(default)]
    pub recent_actions: Vec<String>,
    #[serde(default)]
    pub viewer: ViewerState,
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
            recent_actions: Vec::new(),
            viewer: ViewerState::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_line_numbers_default_to_on_and_old_state_files_still_load() {
        assert!(AppState::default().viewer.show_line_numbers);
        let s: AppState = serde_json::from_str(r#"{"orientation":"horizontal"}"#).unwrap();
        assert!(s.viewer.show_line_numbers);
        let s: AppState = serde_json::from_str(r#"{"viewer":{}}"#).unwrap();
        assert!(s.viewer.show_line_numbers);
        let s: AppState = serde_json::from_str(r#"{"viewer":{"show_line_numbers":false}}"#).unwrap();
        assert!(!s.viewer.show_line_numbers);
    }

    #[test]
    fn viewer_line_numbers_round_trip_under_the_viewer_key() {
        let mut s = AppState::default();
        s.viewer.show_line_numbers = false;
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["viewer"]["show_line_numbers"], false);
        let back: AppState = serde_json::from_value(json).unwrap();
        assert!(!back.viewer.show_line_numbers);
    }
}
