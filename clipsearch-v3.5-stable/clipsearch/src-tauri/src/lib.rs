// ─────────────────────────────────────────────────────────────────────────────
// ClipVault — library entry point.
//
// The crate is split into independent modules, each owning a single concern:
//
//   database  — SQLite + FTS5 connection pool, schema, pragmas
//   clipboard — cross-platform clipboard watcher (polls every 500 ms)
//   obsidian  — vault indexer + filesystem watcher (notify)
//   search    — unified ranked search over both sources
//   settings  — JSON config persisted to the OS config dir
//   hotkeys   — global shortcut registration (default Cmd/Ctrl+Shift+V)
//   window    — show/hide/focus helpers for the floating launcher window
//   ui        — Tauri IPC commands (the only module that talks to the frontend)
//
// Each module exposes a small surface area. The `ui` module is the only one
// that depends on Tauri types beyond `AppHandle`/`State`.
// ─────────────────────────────────────────────────────────────────────────────

pub mod clipboard;
pub mod database;
pub mod files;
pub mod hotkeys;
pub mod obsidian;
pub mod search;
pub mod settings;
pub mod ui;
pub mod web;
pub mod window;

use std::sync::Arc;

use anyhow::Context;
use tauri::Manager;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::database::Database;
use crate::settings::Settings;

/// The shared application state. Cloned cheaply (it's all `Arc`-backed) and
/// handed to every Tauri command via `tauri::State`.
pub struct AppState {
    pub db: Arc<Database>,
    pub settings: Arc<parking_lot::RwLock<Settings>>,
    pub clipboard: Arc<clipboard::ClipboardWatcher>,
    pub obsidian: Arc<parking_lot::Mutex<Option<Arc<obsidian::ObsidianIndexer>>>>,
    pub files: Arc<parking_lot::Mutex<Option<Arc<files::FileIndexer>>>>,
    pub search: Arc<search::SearchEngine>,
}

/// Boot the application.
pub fn run() {
    // ── Tracing ────────────────────────────────────────────────────────────
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,clipvault=debug,sqlx=warn"));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false).with_ansi(true))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec!["--autostart"]),
            )
        )
        .setup(|app| {
            // ── Resolve paths ───────────────────────────────────────────────
            let data_dir = app
                .path()
                .app_data_dir()
                .context("failed to resolve app_data_dir")?;
            std::fs::create_dir_all(&data_dir)
                .with_context(|| format!("creating data dir {data_dir:?}"))?;
            tracing::info!(?data_dir, "data directory");

            // ── Load settings ───────────────────────────────────────────────
            let settings = Settings::load(&data_dir)?;
            let settings = Arc::new(parking_lot::RwLock::new(settings));

            // ── Open the database ───────────────────────────────────────────
            let db_path = data_dir.join("clipvault.db");
            let db = Arc::new(tauri::async_runtime::block_on(Database::open(&db_path))?);

            // ── Start background services ───────────────────────────────────
            let clipboard = clipboard::ClipboardWatcher::start(
                Arc::clone(&db),
                data_dir.clone(),
            )?;
            let search = Arc::new(search::SearchEngine::new(Arc::clone(&db)));

            // ── Obsidian indexer (None until a vault is configured) ─────────
            let obsidian = Arc::new(parking_lot::Mutex::new(None::<Arc<obsidian::ObsidianIndexer>>));
            {
                let s = settings.read().clone();
                if let Some(vault) = s.obsidian_vault_path.as_ref() {
                    match obsidian::ObsidianIndexer::start(
                        std::path::PathBuf::from(vault),
                        Arc::clone(&db),
                        app.handle().clone(),
                    ) {
                        Ok(idx) => *obsidian.lock() = Some(idx),
                        Err(e) => tracing::error!(error = ?e, "failed to start obsidian indexer"),
                    }
                }
            }

            // ── File indexer (uses default folders if none configured) ──────
            // Exclude the Obsidian vault from file indexing — those files are
            // indexed separately as notes (with full-text search + tags).
            let files = Arc::new(parking_lot::Mutex::new(None::<Arc<files::FileIndexer>>));
            {
                let s = settings.read().clone();
                let folders = if s.indexed_folders.is_empty() {
                    default_indexed_folders()
                } else {
                    s.indexed_folders.iter().map(std::path::PathBuf::from).collect()
                };
                let exclusions: Vec<String> = s
                    .obsidian_vault_path
                    .iter()
                    .cloned()
                    .collect();
                if !folders.is_empty() {
                    let idx = files::FileIndexer::start(
                        folders,
                        Arc::clone(&db),
                        app.handle().clone(),
                        exclusions,
                    );
                    *files.lock() = Some(idx);
                }
            }

            // ── Register global hotkey ──────────────────────────────────────
            hotkeys::register(app.handle(), &settings.read())?;

            // ── Enable autostart by default on first run ─────────────────────
            // The autostart plugin manages the OS-level registry/plist entry.
            // We check whether it's already enabled (e.g. after a previous run
            // or after the user explicitly disabled it) and only enable it if
            // it's not already set. This way the user's explicit "disable"
            // choice is preserved across restarts.
            {
                use tauri_plugin_autostart::ManagerExt;
                let autostart = app.autolaunch();
                match autostart.is_enabled() {
                    Ok(false) => {
                        if let Err(e) = autostart.enable() {
                            tracing::warn!(error = ?e, "failed to enable autostart on first run");
                        } else {
                            tracing::info!("autostart enabled (first run default)");
                        }
                    }
                    Ok(true) => {
                        tracing::debug!("autostart already enabled — leaving as-is");
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "failed to query autostart state");
                    }
                }
            }

            // ── Insert shared state ─────────────────────────────────────────
            app.manage(AppState {
                db,
                settings,
                clipboard,
                obsidian,
                files,
                search,
            });

            // ── Tray icon menu ─────────────────────────────────────────────
            window::setup_tray(app.handle())?;

            // ── Window event handlers ──────────────────────────────────────
            window::setup_window_events(app.handle());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // search
            ui::search,
            ui::search_debounced,
            // clipboard
            ui::get_clipboard_history,
            ui::copy_clipboard_item,
            ui::pin_clipboard_item,
            ui::delete_clipboard_item,
            ui::update_clipboard_item,
            ui::update_clipboard_title,
            ui::clear_clipboard_history,
            // obsidian
            ui::set_obsidian_vault,
            ui::get_obsidian_status,
            ui::open_note,
            ui::reveal_note,
            ui::reindex_vault,
            ui::pin_note,
            // favorites (for file/app/web)
            ui::toggle_favorite,
            ui::is_favorited,
            ui::update_favorite_title,
            ui::get_favorites,
            // files
            ui::open_file,
            ui::copy_file_to_clipboard,
            ui::reveal_file,
            ui::reindex_files,
            ui::get_files_status,
            ui::set_indexed_folders,
            // apps — removed
            // web
            ui::open_web_url,
            ui::open_onenote,
            // settings / app
            ui::get_settings,
            ui::update_settings,
            ui::toggle_launcher,
            ui::hide_launcher,
            ui::get_app_version,
            // autostart
            ui::is_autostart_enabled,
            ui::enable_autostart,
            ui::disable_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Return the default folders to index for file search, based on the OS.
/// On Windows: Documents, Desktop, Downloads + all available drive roots
/// (D:\, E:\, etc. — shallow, depth-limited scan).
/// On macOS/Linux: Documents, Desktop, Downloads under $HOME.
fn default_indexed_folders() -> Vec<std::path::PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let candidates = ["Documents", "Desktop", "Downloads"];
    let mut folders: Vec<std::path::PathBuf> = candidates
        .iter()
        .map(|c| home.join(c))
        .filter(|p| p.exists())
        .collect();

    // On Windows, also add all available drive roots (D:\, E:\, etc.)
    // so the user can search files on other drives without manual setup.
    #[cfg(target_os = "windows")]
    {
        for letter in b'D'..=b'Z' {
            let drive = format!("{}:\\", letter as char);
            let path = std::path::PathBuf::from(&drive);
            if path.exists() {
                folders.push(path);
            }
        }
    }

    folders
}
