// ─────────────────────────────────────────────────────────────────────────────
// UI commands — the Tauri IPC layer.
//
// Every function in this module is exposed to the frontend via
// `tauri::generate_handler!` in `lib.rs`. They are thin wrappers around the
// underlying modules:
//
//   • search / search_debounced       -> search::SearchEngine
//   • get_clipboard_history / pin / delete / clear  -> clipboard_repo
//   • copy_clipboard_item             -> writes to the system clipboard
//   • set_obsidian_vault / reindex    -> obsidian::ObsidianIndexer
//   • open_note / reveal_note         -> obsidian:// URI / shell open
//   • get_settings / update_settings  -> settings::Settings
//   • toggle_launcher / hide_launcher -> window
//   • get_obsidian_status             -> ObsidianIndexer::status
//
// All commands are async. They take `tauri::State<AppState>` for shared state
// and return `Result<T, String>` so errors serialize cleanly across IPC.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_shell::ShellExt;

use crate::clipboard::WatcherConfig;
use crate::obsidian::ObsidianIndexer;
use crate::search::SearchResult;
use crate::settings::Settings;
use crate::AppState;

// ── Search ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    state
        .search
        .search(&query, limit.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

/// Same as `search` but with an explicit debounce hint in the name. The
/// frontend debounces; this command exists so the call site reads naturally.
#[tauri::command]
pub async fn search_debounced(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    search(state, query, limit).await
}

// ── Clipboard ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_clipboard_history(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<crate::database::clipboard_repo::ClipboardItem>, String> {
    crate::database::clipboard_repo::recent(state.db.pool(), limit.unwrap_or(100))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn copy_clipboard_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    // Fetch the item — including `kind` and `image_path` so we can handle
    // image clipboard entries correctly.
    let pool = state.db.pool().clone();
    let row = sqlx::query(
        r#"SELECT content, kind, image_path FROM clipboard WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| e.to_string())?;

    let Some(row) = row else {
        return Err(format!("clipboard item {} not found", id));
    };
    let content: String = sqlx::Row::try_get(&row, "content").map_err(|e| e.to_string())?;
    let kind: String = sqlx::Row::try_get(&row, "kind").map_err(|e| e.to_string())?;
    let image_path: Option<String> = sqlx::Row::try_get(&row, "image_path")
        .map_err(|e| e.to_string())?;

    if kind == "image" {
        // Copy the image back to the clipboard using native Win32 API
        // (avoids arboard's "clipboard not open" conflict with our watcher).
        if let Some(path) = image_path {
            let img_result = std::fs::read(&path).map_err(|e| e.to_string());
            match img_result {
                Ok(bytes) => {
                    let blocking = tokio::task::spawn_blocking(move || -> bool {
                        crate::clipboard::set_clipboard_image(&bytes)
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                    if !blocking {
                        tracing::warn!("failed to copy image to clipboard via Win32 API");
                        return Err("Failed to copy image".to_string());
                    }
                }
                Err(e) => return Err(e),
            }
        }
    } else if kind == "files" {
        // Copy the file paths back to the clipboard as CF_HDROP.
        let files: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let blocking = tokio::task::spawn_blocking(move || -> bool {
            crate::clipboard::set_clipboard_files(&files)
        })
        .await
        .map_err(|e| e.to_string())?;
        if !blocking {
            tracing::warn!("failed to copy files to clipboard");
        }
    } else {
        // Text — write via the Tauri clipboard plugin.
        app.clipboard()
            .write_text(&content)
            .map_err(|e| e.to_string())?;
    }

    // Update last_used_at so the item bubbles to the top.
    let now_ms = chrono::Utc::now().timestamp_millis();
    crate::database::clipboard_repo::touch(state.db.pool(), id, now_ms)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn pin_clipboard_item(
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    crate::database::clipboard_repo::set_pinned(state.db.pool(), id, pinned)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_clipboard_item(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    crate::database::clipboard_repo::delete(state.db.pool(), id)
        .await
        .map_err(|e| e.to_string())
}

/// Edit the text content of a clipboard item (for pinned/favorite items).
#[tauri::command]
pub async fn update_clipboard_item(
    state: State<'_, AppState>,
    id: i64,
    content: String,
) -> Result<(), String> {
    crate::database::clipboard_repo::update_content(state.db.pool(), id, content)
        .await
        .map_err(|e| e.to_string())
}

/// Update the custom title of a clipboard item (rename favorites).
#[tauri::command]
pub async fn update_clipboard_title(
    state: State<'_, AppState>,
    id: i64,
    custom_title: Option<String>,
) -> Result<(), String> {
    crate::database::clipboard_repo::update_custom_title(state.db.pool(), id, custom_title)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_clipboard_history(state: State<'_, AppState>) -> Result<u64, String> {
    crate::database::clipboard_repo::clear_unpinned(state.db.pool())
        .await
        .map_err(|e| e.to_string())
}

// ── Obsidian ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn set_obsidian_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<crate::obsidian::IndexerStatus, String> {
    // Stop the existing indexer (if any).
    {
        let mut guard = state.obsidian.lock();
        *guard = None; // dropping the old Arc stops the watcher bridge thread
    }

    // Clear ALL notes from the database — the previous vault's notes are now
    // stale. (delete_by_vault only clears one vault; we want a clean slate
    // so there's no confusion between old and new vault contents.)
    sqlx::query("DELETE FROM notes")
        .execute(state.db.pool())
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!("cleared all notes from DB before re-indexing new vault");

    // Start a new indexer.
    let indexer = ObsidianIndexer::start(
        std::path::PathBuf::from(&path),
        Arc::clone(&state.db),
        app.clone(),
    )
    .map_err(|e| e.to_string())?;

    let status = indexer.status();
    *state.obsidian.lock() = Some(indexer);

    // Persist to settings.
    {
        let mut s = state.settings.write();
        s.obsidian_vault_path = Some(path);
        let _ = s.save(&app.path().config_dir().unwrap_or_default());
    }

    Ok(status)
}

#[tauri::command]
pub async fn get_obsidian_status(
    state: State<'_, AppState>,
) -> Result<crate::obsidian::IndexerStatus, String> {
    let guard = state.obsidian.lock();
    Ok(match guard.as_ref() {
        Some(idx) => idx.status(),
        None => crate::obsidian::IndexerStatus::default(),
    })
}

/// Pin/unpin a note (favorites). Pinned notes always appear at the top.
#[tauri::command]
pub async fn pin_note(
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    crate::database::notes_repo::set_pinned(state.db.pool(), id, pinned)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reindex_vault(state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.obsidian.lock();
    if let Some(idx) = guard.as_ref() {
        idx.reindex_now();
        Ok(())
    } else {
        Err("no vault configured".to_string())
    }
}

/// Open a note via the `obsidian://open` URI scheme.
/// Falls back to opening the file directly via the OS shell if Obsidian is
/// unavailable (the shell will use the default `.md` handler).
///
/// We use the `obsidian://open?path=<absolute_path>` form instead of
/// `?vault=X&file=Y` because it's more robust with non-ASCII filenames,
/// spaces, and nested folders. Obsidian resolves the absolute path directly.
#[tauri::command]
pub async fn open_note(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let note = crate::database::notes_repo::get_by_id(state.db.pool(), id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("note {} not found", id))?;

    // Use the absolute path form — most robust against non-ASCII names,
    // spaces, and nested folders. URL-encode everything that isn't a safe
    // char (including `/` in the path, since the whole path is one value).
    let abs_encoded = url_encode_full(&note.abs_path);
    let uri = format!("obsidian://open?path={abs_encoded}");

    tracing::info!(%uri, abs_path = %note.abs_path, "opening note via obsidian://");

    // Open via the OS default handler. On Windows this invokes the registry
    // handler for `obsidian://`; on macOS the LaunchServices handler; on
    // Linux the xdg-open handler.
    app.shell()
        .open(uri, None)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Reveal the note file in the OS file manager (Finder / Explorer).
#[tauri::command]
pub async fn reveal_note(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let note = crate::database::notes_repo::get_by_id(state.db.pool(), id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("note {} not found", id))?;

    let abs = std::path::PathBuf::from(&note.abs_path);
    if !abs.exists() {
        return Err(format!("file not found: {}", abs.display()));
    }

    // Use the `shell` plugin to reveal in file manager.
    // On Windows: `explorer.exe /select,"<path>"`; on macOS: `open -R`;
    // on Linux: `xdg-open` the parent dir. The shell plugin handles this.
    let parent = abs
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    app.shell()
        .open(parent, None)
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ── Favorites (for file/app/web results) ─────────────────────────────────────

/// Toggle favorite status for a file/app/web result.
#[tauri::command]
pub async fn toggle_favorite(
    state: State<'_, AppState>,
    source: String,
    key: String,
    title: String,
    subtitle: Option<String>,
    custom_title: Option<String>,
) -> Result<bool, String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    // Check if it's already favorited.
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM favorites WHERE source = ? AND key = ?",
    )
    .bind(&source)
    .bind(&key)
    .fetch_optional(state.db.pool())
    .await
    .map_err(|e| e.to_string())?;

    if let Some(_id) = existing {
        // Unfavorite.
        sqlx::query("DELETE FROM favorites WHERE source = ? AND key = ?")
            .bind(&source)
            .bind(&key)
            .execute(state.db.pool())
            .await
            .map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        // Favorite — use custom_title if provided, otherwise the default title.
        let display_title = custom_title.clone().unwrap_or_else(|| title.clone());
        let custom_title_to_store = custom_title.clone();
        sqlx::query(
            "INSERT INTO favorites (source, key, title, subtitle, custom_title, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&source)
        .bind(&key)
        .bind(&display_title)
        .bind(&subtitle)
        .bind(&custom_title_to_store)
        .bind(now_ms)
        .execute(state.db.pool())
        .await
        .map_err(|e| e.to_string())?;
        Ok(true)
    }
}

/// Check if a result is favorited.
#[tauri::command]
pub async fn is_favorited(
    state: State<'_, AppState>,
    source: String,
    key: String,
) -> Result<bool, String> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM favorites WHERE source = ? AND key = ?",
    )
    .bind(&source)
    .bind(&key)
    .fetch_optional(state.db.pool())
    .await
    .map_err(|e| e.to_string())?;
    Ok(existing.is_some())
}

/// Update the custom title of a favorite.
#[tauri::command]
pub async fn update_favorite_title(
    state: State<'_, AppState>,
    source: String,
    key: String,
    new_title: String,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE favorites SET title = ?, custom_title = ? WHERE source = ? AND key = ?",
    )
    .bind(&new_title)
    .bind(&new_title)
    .bind(&source)
    .bind(&key)
    .execute(state.db.pool())
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Get all favorites as SearchResult-like objects.
/// Used to populate the "Избранные" filter tab AND to inject matching
/// favorites into search results (see `mergeFavorites` on the frontend).
///
/// Returns TWO kinds of favorites:
///   1. Rows from the `favorites` table (file/app/web/itsm/onenote).
///   2. Pinned clipboard items (where `clipboard.pinned = 1`). These are
///      NOT in the `favorites` table — they live in the `clipboard` table
///      with `pinned=1`. We surface them here so the frontend can search
///      by custom_title (e.g. a pinned URL whose content is the URL itself
///      but whose custom_title is "Привет" should be findable by "Привет").
#[tauri::command]
pub async fn get_favorites(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let rows = sqlx::query(
        "SELECT source, key, title, subtitle FROM favorites ORDER BY created_at DESC",
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        let source: String = sqlx::Row::try_get(&row, "source").map_err(|e| e.to_string())?;
        let key: String = sqlx::Row::try_get(&row, "key").map_err(|e| e.to_string())?;
        let title: String = sqlx::Row::try_get(&row, "title").map_err(|e| e.to_string())?;
        let subtitle: Option<String> = sqlx::Row::try_get(&row, "subtitle").ok();

        // Build a JSON object matching the SearchResult format.
        let obj = match source.as_str() {
            "file" => {
                // Extract file extension from the path for correct icon display.
                let ext = std::path::Path::new(&key)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                serde_json::json!({
                    "source": "file",
                    "path": key,
                    "name": title,
                    "ext": ext,
                    "is_dir": false,
                    "size": 0,
                    "modified_at": 0,
                    "score": 1.0
                })
            }
            "app" => serde_json::json!({
                "source": "app",
                "name": title,
                "exec_path": key,
                "bundle_id": null,
                "version": null,
                "icon_path": null,
                "score": 1.0
            }),
            "web" => serde_json::json!({
                "source": "web",
                "query": title,
                "provider": subtitle.unwrap_or_default(),
                "url": key,
                "display_text": title,
                "score": 1.0
            }),
            "itsm" => serde_json::json!({
                "source": "itsm",
                "query": title,
                "url": key,
                "score": 1.0
            }),
            _ => continue,
        };
        out.push(obj);
    }

    // ── Also surface pinned clipboard items ──────────────────────────────────
    // These are clipboard entries with `pinned = 1`. They're "favorites" in
    // the UI sense (the user pressed Ctrl+P to pin them), but they live in
    // the `clipboard` table, not the `favorites` table. Without this, a
    // pinned URL whose content is the URL string but whose custom_title is
    // a friendly name (e.g. "Привет") would never be found by searching the
    // friendly name — the FTS5 index only matches content, not custom_title.
    let pinned_rows = sqlx::query(
        r#"
        SELECT id, content, kind, byte_size, source_app, source_app_name,
               created_at, last_used_at, use_count, image_path, custom_title
        FROM clipboard
        WHERE pinned = 1
        ORDER BY last_used_at DESC
        "#,
    )
    .fetch_all(state.db.pool())
    .await
    .map_err(|e| e.to_string())?;

    for row in pinned_rows {
        let id: i64 = sqlx::Row::try_get(&row, "id").map_err(|e| e.to_string())?;
        let content: String = sqlx::Row::try_get(&row, "content").map_err(|e| e.to_string())?;
        let kind: String = sqlx::Row::try_get(&row, "kind").map_err(|e| e.to_string())?;
        let byte_size: i64 = sqlx::Row::try_get(&row, "byte_size").unwrap_or(0);
        let source_app: Option<String> = sqlx::Row::try_get(&row, "source_app").ok();
        let source_app_name: Option<String> = sqlx::Row::try_get(&row, "source_app_name").ok();
        let created_at: i64 = sqlx::Row::try_get(&row, "created_at").unwrap_or(0);
        let last_used_at: i64 = sqlx::Row::try_get(&row, "last_used_at").unwrap_or(0);
        let use_count: i64 = sqlx::Row::try_get(&row, "use_count").unwrap_or(0);
        let image_path: Option<String> = sqlx::Row::try_get(&row, "image_path").ok();
        let custom_title: Option<String> = sqlx::Row::try_get(&row, "custom_title").ok();

        // Backfill image_path for OLD rows where the watcher didn't set it
        // (legacy rows from before the v2 fix). If kind = "files" and the
        // first file path looks like an image, use that path as image_path.
        let image_path_final = image_path.clone().or_else(|| {
            if kind == "files" {
                content.lines().next().and_then(|first_path| {
                    let lower = first_path.to_lowercase();
                    const IMG_EXTS: &[&str] = &[
                        ".jpg", ".jpeg", ".png", ".gif", ".webp",
                        ".bmp", ".svg", ".tiff", ".tif", ".ico",
                    ];
                    if IMG_EXTS.iter().any(|e| lower.ends_with(e)) {
                        Some(first_path.to_string())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        });

        // Title = custom_title if set, else first NON-EMPTY line of content.
        // Using `lines().next()` was buggy — if content starts with "\n"
        // (which happens when the user copies text with a leading newline),
        // `lines().next()` returns Some("") and the title becomes empty,
        // causing "(empty)" to show in the favorites list.
        let title = custom_title.clone().unwrap_or_else(|| {
            content
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .to_string()
        });

        // Preview = first 200 chars of content (for matching + UI display).
        let preview: String = content.chars().take(200).collect();

        out.push(serde_json::json!({
            "source": "clipboard",
            "id": id,
            "title": title,
            "preview": preview,
            "content": content,
            "kind": kind,
            "source_app": source_app,
            "source_app_name": source_app_name,
            "created_at": created_at,
            "last_used_at": last_used_at,
            "pinned": true,
            "use_count": use_count,
            "image_path": image_path_final,
            "custom_title": custom_title,
            "byte_size": byte_size,
            "score": 1.0
        }));
    }

    Ok(out)
}

// ── Files ────────────────────────────────────────────────────────────────────

/// Open a file with its default associated application.
#[tauri::command]
pub async fn open_file(app: AppHandle, path: String) -> Result<(), String> {
    app.shell()
        .open(path, None)
        .map_err(|e| e.to_string())
}

/// Copy a single file to the clipboard (as CF_HDROP, like Ctrl+C in Explorer).
#[tauri::command]
pub async fn copy_file_to_clipboard(path: String) -> Result<(), String> {
    let files = vec![path];
    let blocking = tokio::task::spawn_blocking(move || -> bool {
        crate::clipboard::set_clipboard_files(&files)
    })
    .await
    .map_err(|e| e.to_string())?;
    if !blocking {
        return Err("Failed to copy file to clipboard".to_string());
    }
    Ok(())
}

/// Reveal a file in the OS file manager (Explorer / Finder).
#[tauri::command]
pub async fn reveal_file(app: AppHandle, path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    let parent = p
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(path);
    app.shell()
        .open(parent, None)
        .map_err(|e| e.to_string())
}

/// Force a full re-scan of indexed folders.
#[tauri::command]
pub async fn reindex_files(state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.files.lock();
    if let Some(idx) = guard.as_ref() {
        idx.reindex_now();
        Ok(())
    } else {
        Err("file indexer not running".to_string())
    }
}

#[tauri::command]
pub async fn get_files_status(
    state: State<'_, AppState>,
) -> Result<crate::files::FilesStatus, String> {
    let guard = state.files.lock();
    Ok(match guard.as_ref() {
        Some(idx) => idx.status(),
        None => crate::files::FilesStatus::default(),
    })
}

/// Update the list of indexed folders. Triggers a full re-scan.
#[tauri::command]
pub async fn set_indexed_folders(
    app: AppHandle,
    state: State<'_, AppState>,
    folders: Vec<String>,
) -> Result<(), String> {
    // Stop existing indexer.
    {
        let mut guard = state.files.lock();
        *guard = None;
    }
    // Persist to settings.
    {
        let mut s = state.settings.write();
        s.indexed_folders = folders.clone();
        let _ = s.save(&app.path().config_dir().unwrap_or_default());
    }
    // Start a new indexer. Exclude the Obsidian vault (if configured) — those
    // files are indexed as notes, not as plain files.
    let exclusions: Vec<String> = {
        let s = state.settings.read();
        s.obsidian_vault_path.iter().cloned().collect()
    };
    let folders: Vec<std::path::PathBuf> =
        folders.into_iter().map(std::path::PathBuf::from).collect();
    if !folders.is_empty() {
        let idx = crate::files::FileIndexer::start(
            folders,
            Arc::clone(&state.db),
            app,
            exclusions,
        );
        *state.files.lock() = Some(idx);
    }
    Ok(())
}

// ── Apps — removed ─────────────────────────────────────────────────────────

// ── Web ──────────────────────────────────────────────────────────────────────

/// Open a web URL in the default browser.
#[tauri::command]
pub async fn open_web_url(app: AppHandle, url: String) -> Result<(), String> {
    app.shell()
        .open(url, None)
        .map_err(|e| e.to_string())
}

/// Open a OneNote .one file in OneNote via the onenote:// URI scheme.
#[tauri::command]
pub async fn open_onenote(app: AppHandle, path: String) -> Result<(), String> {
    // Build onenote:// URI — OneNote will open the file directly.
    let uri = format!("onenote:///{}", path.replace('\\', "/"));
    app.shell()
        .open(uri, None)
        .map_err(|e| e.to_string())
}

// ── Settings / app ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().clone())
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<Settings, String> {
    let mut s = state.settings.write();
    let current = s.clone();
    let mut current_value = serde_json::to_value(&current).map_err(|e| e.to_string())?;
    merge_json(&mut current_value, patch);
    *s = serde_json::from_value(current_value).map_err(|e| e.to_string())?;
    let new_settings = s.clone();

    // Persist to disk.
    let config_dir = app.path().config_dir().unwrap_or_default();
    let _ = s.save(&config_dir);
    drop(s);

    // Apply side effects:
    // - Hotkey changed -> re-register.
    if let Err(e) = crate::hotkeys::refresh(&app, &new_settings) {
        tracing::warn!(error = ?e, "failed to refresh hotkeys");
    }
    // - Clipboard watcher config changed.
    state.clipboard.set_config(WatcherConfig {
        poll_ms: new_settings.clipboard_poll_ms,
        history_limit: new_settings.clipboard_history_limit,
    });
    // - Web search provider changed.
    state
        .search
        .set_web_provider(&new_settings.web_search_provider);
    // - Enabled sources changed.
    state.search.set_enabled_sources(crate::search::EnabledSources {
        clipboard: new_settings.enabled_sources.clipboard,
        obsidian: new_settings.enabled_sources.obsidian,
        files: new_settings.enabled_sources.files,
        web: new_settings.enabled_sources.web,
        itsm: new_settings.enabled_sources.itsm,
        onenote: new_settings.enabled_sources.onenote,
        confl: new_settings.enabled_sources.confl,
        jira: new_settings.enabled_sources.jira,
    });
    // - Theme changed -> apply to all windows.
    let theme = match new_settings.theme.as_str() {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    };
    if let Some(theme) = theme {
        for win in app.webview_windows().values() {
            let _ = win.set_theme(Some(theme.clone()));
        }
    }
    // - Window size changed.
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_size(tauri::LogicalSize::new(
            new_settings.window_width,
            new_settings.window_height,
        ));
    }

    Ok(new_settings)
}

#[tauri::command]
pub fn toggle_launcher(app: AppHandle) {
    crate::window::toggle(&app);
}

#[tauri::command]
pub fn hide_launcher(app: AppHandle) {
    crate::window::hide_launcher(&app);
}

#[tauri::command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

// ── Autostart ────────────────────────────────────────────────────────────────
//
// The autostart plugin writes a registry entry on Windows
// (HKCU\Software\Microsoft\Windows\CurrentVersion\Run) or a LaunchAgent plist
// on macOS. We expose three small commands so the Settings panel can read
// the current state and toggle it on/off.
//
// The state is owned by the OS (registry / plist), not by our settings.json —
// this way the toggle stays in sync with what Windows actually does on boot.

#[tauri::command]
pub fn is_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    autostart.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn enable_autostart(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    autostart.enable().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn disable_autostart(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    autostart.disable().map_err(|e| e.to_string())
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Recursively merge `patch` into `target`. Objects are merged key-by-key;
/// non-objects are replaced.
fn merge_json(target: &mut serde_json::Value, patch: serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge_json(t.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (t, p) => *t = p,
    }
}

/// Minimal URL-encoder for path components. Encodes everything except
/// unreserved characters (`A-Za-z0-9-_.~`) and `/` (preserved as path sep).
fn url_encode(s: &str) -> String {
    url_encode_impl(s, true)
}

/// URL-encoder that also escapes `/` — used for the `path=` form of
/// `obsidian://open` where the entire value is a single path segment.
fn url_encode_full(s: &str) -> String {
    url_encode_impl(s, false)
}

fn url_encode_impl(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_preserves_unreserved() {
        assert_eq!(url_encode("Hello-World_42.7~"), "Hello-World_42.7~");
    }

    #[test]
    fn url_encode_escapes_spaces() {
        assert_eq!(url_encode("my note"), "my%20note");
    }

    #[test]
    fn url_encode_preserves_slashes() {
        assert_eq!(url_encode("dir/sub dir/note"), "dir/sub%20dir/note");
    }

    #[test]
    fn merge_json_deep_merges_objects() {
        let mut t = serde_json::json!({"a": 1, "b": {"c": 2}});
        merge_json(&mut t, serde_json::json!({"b": {"d": 3}}));
        assert_eq!(t, serde_json::json!({"a": 1, "b": {"c": 2, "d": 3}}));
    }
}
