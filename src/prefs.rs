use crate::model::{SortBy, SortOrder, ThemeMode, ViewMode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPrefs {
    pub theme: ThemeMode,
    pub view_mode: ViewMode,
    pub sort_by: SortBy,
    pub sort_order: SortOrder,
    pub show_hidden: bool,
    pub folders_first: bool,
    pub dual_pane: bool,
    pub show_inspector: bool,
    pub favorites: Vec<PathBuf>,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            view_mode: ViewMode::List,
            sort_by: SortBy::Name,
            sort_order: SortOrder::Ascending,
            show_hidden: false,
            folders_first: true,
            dual_pane: false,
            show_inspector: false,
            favorites: Vec::new(),
        }
    }
}

impl AppPrefs {
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join("prefs.json");
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) {
        let _ = fs::create_dir_all(config_dir);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(config_dir.join("prefs.json"), json);
        }
    }
}
