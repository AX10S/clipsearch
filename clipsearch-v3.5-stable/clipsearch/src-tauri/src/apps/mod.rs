// ─────────────────────────────────────────────────────────────────────────────
// Apps module — find installed applications on the current OS.
//
// Unlike clipboard/notes, we don't persist apps to SQLite — the list is small
// (typically 100-500 entries) and changes rarely. We scan on startup, cache
// in a `DashMap`, and refresh every hour.
//
// Platform scanning:
//   Windows: Program Files, Program Files (x86), LOCALAPPDATA\Microsoft\WindowsApps
//   macOS:   /Applications, ~/Applications, /System/Applications
//   Linux:   /usr/share/applications/*.desktop, ~/.local/share/applications/*.desktop
// ─────────────────────────────────────────────────────────────────────────────

pub mod platform;

use std::sync::Arc;

use parking_lot::RwLock;

/// One installed application.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppInfo {
    pub name: String,
    pub exec_path: String,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub icon_path: Option<String>,
}

/// The app cache. Held in `Arc` inside `AppState`.
pub struct AppIndex {
    apps: RwLock<Vec<AppInfo>>,
}

impl AppIndex {
    /// Build an empty index. Call `refresh()` to populate it.
    pub fn new() -> Arc<Self> {
        let idx = Arc::new(Self {
            apps: RwLock::new(Vec::new()),
        });
        idx.spawn_refresh_task();
        idx
    }

    /// Spawn a background task that refreshes the index on startup and then
    /// every hour. The task lives for the lifetime of the process.
    fn spawn_refresh_task(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            // Initial scan.
            if let Some(idx) = weak.upgrade() {
                idx.refresh().await;
            }
            // Refresh every hour.
            let mut tick =
                tokio::time::interval(std::time::Duration::from_secs(3600));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                if let Some(idx) = weak.upgrade() {
                    idx.refresh().await;
                } else {
                    break; // AppIndex dropped — exit.
                }
            }
        });
    }

    /// Scan the system for installed apps and update the cache.
    pub async fn refresh(self: &Arc<Self>) {
        let apps = tokio::task::spawn_blocking(|| platform::scan_installed_apps())
            .await
            .unwrap_or_default();
        tracing::info!(count = apps.len(), "app index refreshed");
        *self.apps.write() = apps;
    }

    /// Search the app cache by name. Returns up to `limit` matches, sorted by
    /// relevance (prefix match > substring match > alphabetical).
    pub fn search(&self, query: &str, limit: usize) -> Vec<AppInfo> {
        let apps = self.apps.read();
        if query.trim().is_empty() {
            return apps.iter().take(limit).cloned().collect();
        }
        let q = query.to_lowercase();

        let mut scored: Vec<(u8, &AppInfo)> = apps
            .iter()
            .filter_map(|app| {
                let name_lower = app.name.to_lowercase();
                if name_lower == q {
                    Some((4, app))
                } else if name_lower.starts_with(&q) {
                    Some((3, app))
                } else if name_lower.contains(&q) {
                    Some((2, app))
                } else if app
                    .exec_path
                    .to_lowercase()
                    .contains(&q)
                {
                    Some((1, app))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score desc, then by name asc.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
        scored.into_iter().take(limit).map(|(_, a)| a.clone()).collect()
    }

    /// Total number of cached apps (for the status panel).
    pub fn count(&self) -> usize {
        self.apps.read().len()
    }
}

impl Default for AppIndex {
    fn default() -> Self {
        Self {
            apps: RwLock::new(Vec::new()),
        }
    }
}
