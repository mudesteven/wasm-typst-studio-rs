use leptos::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use crate::models::Project;
use crate::storage::traits::ProjectStorage;
use crate::packages::PackageCache;

/// Home page tab
#[derive(Clone, Debug, PartialEq)]
pub enum HomeTab {
    Projects,
    Packages,
    Settings,
}

/// Theme mode
#[derive(Clone, Debug, PartialEq)]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

impl ThemeMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "light" => Self::Light,
            "system" => Self::System,
            _ => Self::Dark,
        }
    }

    pub fn to_str(&self) -> &str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }

    /// Resolve the actual theme name for the data-theme attribute
    pub fn resolve(&self) -> &str {
        match self {
            Self::Dark => "dark",
            Self::Light => "business",
            Self::System => {
                if system_prefers_dark() { "dark" } else { "business" }
            }
        }
    }
}

/// Check system dark mode preference via matchMedia
fn system_prefers_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .map(|mql| mql.matches())
        .unwrap_or(true)
}

/// Central application state provided via Leptos context.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<Box<dyn ProjectStorage + Send + Sync>>,

    // Project state
    pub current_project: RwSignal<Option<Project>>,
    pub project_files: RwSignal<Vec<String>>,

    // Editor state
    pub active_file: RwSignal<Option<String>>,
    pub open_files: RwSignal<Vec<String>>,
    pub file_contents: RwSignal<HashMap<String, String>>,
    pub modified_files: RwSignal<std::collections::HashSet<String>>,

    // Image cache
    pub image_cache: RwSignal<HashMap<String, String>>,

    // UI state
    pub sidebar_visible: RwSignal<bool>,
    pub sidebar_width: RwSignal<f64>,
    pub show_project_manager: RwSignal<bool>,
    pub show_settings: RwSignal<bool>,

    // Packages
    pub package_cache: PackageCache,

    // Settings
    pub autosave_enabled: RwSignal<bool>,
    pub theme_mode: RwSignal<ThemeMode>,
    pub editor_font_size: RwSignal<u32>,

    // Home page navigation
    pub home_tab: RwSignal<HomeTab>,
}

impl AppState {
    pub fn new(storage: Box<dyn ProjectStorage + Send + Sync>) -> Self {
        let autosave = load_bool_setting("autosave_enabled", true);
        let theme = ThemeMode::from_str(
            &load_string_setting("theme_mode").unwrap_or_else(|| "system".to_string())
        );

        Self {
            storage: Arc::new(storage),
            current_project: RwSignal::new(None),
            project_files: RwSignal::new(Vec::new()),
            active_file: RwSignal::new(None),
            open_files: RwSignal::new(Vec::new()),
            file_contents: RwSignal::new(HashMap::new()),
            modified_files: RwSignal::new(std::collections::HashSet::new()),
            image_cache: RwSignal::new(HashMap::new()),
            sidebar_visible: RwSignal::new(true),
            sidebar_width: RwSignal::new(220.0),
            show_project_manager: RwSignal::new(false),
            show_settings: RwSignal::new(false),
            package_cache: PackageCache::new(),
            autosave_enabled: RwSignal::new(autosave),
            theme_mode: RwSignal::new(theme),
            home_tab: RwSignal::new(HomeTab::Projects),
            editor_font_size: RwSignal::new(
                load_string_setting("editor_font_size")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(14)
            ),
        }
    }

    pub fn set_file_content(&self, path: &str, content: String) {
        self.file_contents.update(|map| { map.insert(path.to_string(), content); });
        self.modified_files.update(|set| { set.insert(path.to_string()); });
    }

    pub fn open_file(&self, path: &str) {
        self.open_files.update(|files| {
            if !files.contains(&path.to_string()) {
                files.push(path.to_string());
            }
        });
        self.active_file.set(Some(path.to_string()));
    }

    pub fn close_file(&self, path: &str) {
        let path_str = path.to_string();
        self.open_files.update(|files| { files.retain(|f| f != &path_str); });
        if self.active_file.get_untracked().as_deref() == Some(path) {
            let files = self.open_files.get_untracked();
            self.active_file.set(files.last().cloned());
        }
    }

    pub fn save_last_project_id(project_id: &str) {
        save_string_setting("last_project_id", project_id);
    }

    pub fn load_last_project_id() -> Option<String> {
        load_string_setting("last_project_id")
    }
}

// --- localStorage helpers ---

pub fn load_string_setting(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
        .filter(|v| !v.is_empty())
}

pub fn save_string_setting(key: &str, value: &str) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item(key, value);
        }
    }
}

fn load_bool_setting(key: &str, default: bool) -> bool {
    load_string_setting(key)
        .map(|v| v == "true")
        .unwrap_or(default)
}
