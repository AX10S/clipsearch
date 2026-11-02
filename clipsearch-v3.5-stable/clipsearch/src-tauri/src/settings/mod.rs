// ─────────────────────────────────────────────────────────────────────────────
// Settings — JSON config persisted to the OS config dir.
//
// The file is small and read-mostly: writes happen only when the user changes
// a setting via the UI. We keep an in-memory copy behind a `RwLock` and write
// through to disk on every mutation.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Absolute path to the Obsidian vault directory. `None` until configured.
    #[serde(default)]
    pub obsidian_vault_path: Option<String>,

    /// Global hotkey, in Tauri's accelerator format. Default `CmdOrCtrl+Shift+V`.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// Maximum number of clipboard items to keep. Pinned items are exempt.
    #[serde(default = "default_history_limit")]
    pub clipboard_history_limit: i64,

    /// UI theme: "system" | "light" | "dark".
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Polling interval for the clipboard watcher, in milliseconds.
    #[serde(default = "default_poll_ms")]
    pub clipboard_poll_ms: u64,

    /// Window width in pixels.
    #[serde(default = "default_width")]
    pub window_width: f64,

    /// Window height in pixels.
    #[serde(default = "default_height")]
    pub window_height: f64,

    /// Whether the launcher window should appear on the active monitor's
    /// current workspace (true) or float across all spaces (false, macOS only).
    #[serde(default = "default_true")]
    pub show_on_active_space: bool,

    /// Folders to index for the file search. Empty = use platform defaults.
    #[serde(default)]
    pub indexed_folders: Vec<String>,

    /// Web search provider: "google" | "bing" | "duckduckgo" | "yandex".
    #[serde(default = "default_web_provider")]
    pub web_search_provider: String,

    /// Which sources are enabled.
    #[serde(default = "default_enabled_sources")]
    pub enabled_sources: EnabledSourcesConfig,

    /// Compact mode — show more rows in the result list by using smaller
    /// row height + smaller icon size. Default TRUE: the small/standard
    /// layout is the default on launch; the user can switch to the larger
    /// "Крупный" layout in Settings if desired.
    #[serde(default = "default_true_bool")]
    pub compact_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnabledSourcesConfig {
    #[serde(default = "default_true_bool")]
    pub clipboard: bool,
    #[serde(default = "default_true_bool")]
    pub obsidian: bool,
    #[serde(default = "default_true_bool")]
    pub files: bool,
    #[serde(default = "default_true_bool")]
    pub web: bool,
    #[serde(default = "default_true_bool")]
    pub itsm: bool,
    #[serde(default = "default_true_bool")]
    pub onenote: bool,
    #[serde(default = "default_true_bool")]
    pub confl: bool,
    #[serde(default = "default_true_bool")]
    pub jira: bool,
}

fn default_true_bool() -> bool {
    true
}

fn default_web_provider() -> String {
    "google".to_string()
}

fn default_enabled_sources() -> EnabledSourcesConfig {
    EnabledSourcesConfig {
        clipboard: true,
        obsidian: true,
        files: true,
        web: true,
        itsm: true,
        onenote: true,
        confl: true,
        jira: true,
    }
}

fn default_true() -> bool {
    true
}

fn default_hotkey() -> String {
    "CmdOrCtrl+Shift+Space".to_string()
}
fn default_history_limit() -> i64 {
    100
}
fn default_theme() -> String {
    "dark".to_string()
}
fn default_poll_ms() -> u64 {
    500
}
fn default_width() -> f64 {
    1460.0
}
fn default_height() -> f64 {
    1000.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            obsidian_vault_path: None,
            hotkey: default_hotkey(),
            clipboard_history_limit: default_history_limit(),
            theme: default_theme(),
            clipboard_poll_ms: default_poll_ms(),
            window_width: default_width(),
            window_height: default_height(),
            show_on_active_space: true,
            indexed_folders: Vec::new(),
            web_search_provider: default_web_provider(),
            enabled_sources: default_enabled_sources(),
            compact_mode: true,
        }
    }
}

impl Settings {
    /// Load settings from `<config_dir>/settings.json`. If the file doesn't
    /// exist or fails to parse, returns `Default::default()` and writes a
    /// fresh file so the user has something to edit.
    pub fn load(config_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(config_dir).ok();
        let path = config_dir.join("settings.json");

        if !path.exists() {
            let default = Settings::default();
            let _ = default.save(config_dir);
            return Ok(default);
        }

        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading settings file {}", path.display()))?;

        let settings: Settings = serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(error = ?e, "settings.json parse failed — using defaults");
            Settings::default()
        });

        Ok(settings)
    }

    /// Persist settings to `<config_dir>/settings.json` atomically.
    pub fn save(&self, config_dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(config_dir).ok();
        let path = config_dir.join("settings.json");
        let tmp = path.with_extension("json.tmp");

        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, text)
            .with_context(|| format!("writing tmp file {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s.hotkey, back.hotkey);
        assert_eq!(s.clipboard_history_limit, back.clipboard_history_limit);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let json = r#"{"hotkey": "CmdOrCtrl+P"}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.hotkey, "CmdOrCtrl+P");
        assert_eq!(s.clipboard_history_limit, 1_000);
        assert_eq!(s.theme, "system");
    }
}
