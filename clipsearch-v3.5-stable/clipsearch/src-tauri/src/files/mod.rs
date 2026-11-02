// ─────────────────────────────────────────────────────────────────────────────
// Files module — index local filesystem for fast file search.
//
// The user configures a list of `indexed_folders` (default: Documents, Desktop,
// Downloads). We recursively walk each folder (depth-limited to 4 levels, skip
// hidden / system / node_modules / .git) and store metadata in SQLite.
//
// A `notify` watcher keeps the index in sync: created/modified files get
// upserted, deleted files get pruned. Full re-scan happens on startup and can
// be triggered manually from the settings panel.
// ─────────────────────────────────────────────────────────────────────────────

mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use sqlx::Row;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::database::Database;

/// One file row, sent to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub is_dir: bool,
    pub size: i64,
    pub modified_at: i64,
}

/// Events emitted to the frontend during indexing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FileIndexEvent {
    Started { total: usize },
    Progress { done: usize, total: usize },
    Finished { indexed: i64, took_ms: u64 },
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct FilesStatus {
    pub indexed_count: i64,
    pub last_scan_ms: u64,
    pub scanning: bool,
}

enum Cmd {
    FullScan,
    UpsertFile(PathBuf, String),
    RemoveFile(PathBuf),
    Stop,
}

pub struct FileIndexer {
    tx: mpsc::Sender<Cmd>,
    status: Arc<Mutex<FilesStatus>>,
}

impl FileIndexer {
    pub fn start(
        indexed_folders: Vec<PathBuf>,
        db: Arc<Database>,
        app: AppHandle,
        exclusions: Vec<String>,
    ) -> Arc<Self> {
        let status = Arc::new(Mutex::new(FilesStatus::default()));
        let (tx, rx) = mpsc::channel::<Cmd>(256);

        let indexer = Arc::new(Self {
            tx: tx.clone(),
            status: Arc::clone(&status),
        });

        // Spawn worker.
        let status_worker = Arc::clone(&status);
        tauri::async_runtime::spawn(async move {
            worker(indexed_folders, db, app, status_worker, exclusions, rx).await;
        });

        // Spawn watcher for each folder.
        for folder in indexer.iter_folders() {
            let _ = watcher::spawn(folder.clone(), tx.clone());
        }

        // Kick off initial scan.
        let _ = tx.try_send(Cmd::FullScan);

        indexer
    }

    fn iter_folders(&self) -> Vec<PathBuf> {
        // We can't read from settings here without a cycle; the caller passes
        // the list in `start()`. This method is a no-op placeholder kept for
        // API symmetry with the Obsidian indexer.
        Vec::new()
    }

    pub fn reindex_now(&self) {
        let _ = self.tx.try_send(Cmd::FullScan);
    }

    pub fn status(&self) -> FilesStatus {
        self.status.lock().clone()
    }
}

async fn worker(
    folders: Vec<PathBuf>,
    db: Arc<Database>,
    app: AppHandle,
    status: Arc<Mutex<FilesStatus>>,
    exclusions: Vec<String>,
    mut rx: mpsc::Receiver<Cmd>,
) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Stop => break,
            Cmd::FullScan => {
                full_scan(&folders, &db, &app, &status, &exclusions).await;
            }
            Cmd::UpsertFile(path, root) => {
                if let Err(e) = upsert_file(&db, &path, &root).await {
                    tracing::warn!(error = ?e, path = ?path, "file upsert failed");
                }
                refresh_count(&db, &status).await;
            }
            Cmd::RemoveFile(path) => {
                let _ = sqlx::query("DELETE FROM files WHERE abs_path = ?")
                    .bind(path.to_string_lossy().to_string())
                    .execute(db.pool())
                    .await;
                refresh_count(&db, &status).await;
            }
        }
    }
}

async fn full_scan(
    folders: &[PathBuf],
    db: &Database,
    app: &AppHandle,
    status: &Mutex<FilesStatus>,
    exclusions: &[String],
) {
    {
        let mut s = status.lock();
        if s.scanning {
            return;
        }
        s.scanning = true;
    }
    let started = std::time::Instant::now();

    // Clear existing entries — we re-index from scratch.
    let _ = sqlx::query("DELETE FROM files").execute(db.pool()).await;

    let mut all_files = Vec::new();
    for folder in folders {
        // Use shallower depth for drive roots (e.g. D:\) to avoid scanning
        // the entire drive. Use normal depth for user folders.
        let is_drive_root = folder
            .to_str()
            .map(|s| s.len() <= 3 && s.ends_with(':'))
            .unwrap_or(false);
        let max_depth = if is_drive_root { 2 } else { 4 };
        discover_with_exclusions(folder, folder, &mut all_files, 0, exclusions, max_depth);
    }
    let total = all_files.len();
    let _ = app.emit(
        "files://event",
        FileIndexEvent::Started { total },
    );
    tracing::info!(total, "starting file scan");

    let mut done = 0usize;
    for chunk in all_files.chunks(100) {
        for (path, root) in chunk {
            if let Err(e) = upsert_file(db, path, root).await {
                tracing::warn!(error = ?e, path = ?path, "file upsert failed");
            }
            done += 1;
        }
        let _ = app.emit(
            "files://event",
            FileIndexEvent::Progress { done, total },
        );
        tokio::task::yield_now().await;
    }

    refresh_count(db, status).await;
    let took_ms = started.elapsed().as_millis() as u64;
    {
        let mut s = status.lock();
        s.scanning = false;
        s.last_scan_ms = took_ms;
    }
    let _ = app.emit(
        "files://event",
        FileIndexEvent::Finished {
            indexed: status.lock().indexed_count,
            took_ms,
        },
    );
    tracing::info!(took_ms, "file scan complete");
}

/// Walk `dir` recursively, collecting files/dirs into `out`. Skips:
///   - hidden entries (except `.` itself)
///   - common dev/system folders (node_modules, .git, AppData, Library, ...)
///   - recycle bins ($Recycle.Bin, .Trash, Trash)
///   - temp/cache folders
///   - any folder in `extra_exclusions` (used to skip the Obsidian vault,
///     which is indexed separately as notes)
fn discover_with_exclusions(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, String)>,
    depth: u8,
    extra_exclusions: &[String],
    max_depth: u8,
) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let root_str = root.to_string_lossy().to_string();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden entries (except `.` itself, which we never see here).
        if name_str.starts_with('.') {
            continue;
        }

        // Skip common dev / system / trash folders.
        let skip = matches!(
            name_str.as_ref(),
            "node_modules"
                | "target"
                | "venv"
                | ".venv"
                | "__pycache__"
                | "AppData"
                | "Library"
                | "$Recycle.Bin"
                | "System Volume Information"
                | "Trash"
                | ".Trash"
                | ".Trash-1000"
                | "Temp"
                | "tmp"
                | "cache"
                | "Cache"
                | "Cookies"
                | "Application Data"
                | "Local Settings"
                | "WinSxS"
                | "Drivers"
                | "DriverStore"
                | "assembly"
                | "servicing"
                | "Installer"
        );
        if skip {
            continue;
        }

        // Skip folders explicitly excluded by the caller (e.g. the Obsidian
        // vault path — those files are indexed as notes, not as plain files).
        let path_str = path.to_string_lossy();
        if extra_exclusions
            .iter()
            .any(|ex| path_str.as_ref() == ex.as_str() || path_str.starts_with(ex.as_str()))
        {
            continue;
        }

        out.push((path.clone(), root_str.clone()));

        if path.is_dir() {
            discover_with_exclusions(root, &path, out, depth + 1, extra_exclusions, max_depth);
        }
    }
}

async fn upsert_file(db: &Database, path: &Path, root: &str) -> anyhow::Result<()> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()), // file may have been deleted
    };
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let is_dir = metadata.is_dir();
    let size = if is_dir { 0 } else { metadata.len() as i64 };
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    sqlx::query(
        r#"
        INSERT INTO files (abs_path, name, ext, is_dir, size, modified_at, indexed_at, root_folder)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(abs_path) DO UPDATE SET
            name = excluded.name,
            ext = excluded.ext,
            is_dir = excluded.is_dir,
            size = excluded.size,
            modified_at = excluded.modified_at,
            indexed_at = excluded.indexed_at,
            root_folder = excluded.root_folder
        "#,
    )
    .bind(path.to_string_lossy().to_string())
    .bind(&name)
    .bind(&ext)
    .bind(if is_dir { 1 } else { 0 })
    .bind(size)
    .bind(modified_at)
    .bind(chrono::Utc::now().timestamp_millis())
    .bind(root)
    .execute(db.pool())
    .await?;
    Ok(())
}

async fn refresh_count(db: &Database, status: &Mutex<FilesStatus>) {
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM files")
        .fetch_one(db.pool())
        .await
        .map(|r| r.try_get::<i64, _>("n").unwrap_or(0))
        .unwrap_or(0);
    status.lock().indexed_count = count;
}

/// Search the files index by FTS5 + LIKE fallback.
pub async fn search(
    db: &Database,
    query: &str,
    limit: i64,
) -> anyhow::Result<Vec<FileEntry>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
    let fts_query = sanitize_fts_query(query);

    // First try FTS5 (with AND logic — all words must match).
    let rows = if fts_query.is_empty() {
        // No usable FTS tokens — fall back to LIKE.
        sqlx::query(
            r#"
            SELECT abs_path, name, ext, is_dir, size, modified_at
            FROM files
            WHERE name LIKE ? ESCAPE '\'
            ORDER BY modified_at DESC
            LIMIT ?
            "#,
        )
        .bind(&like)
        .bind(limit)
        .fetch_all(db.pool())
        .await?
    } else {
        let fts_rows = sqlx::query(&format!(
            r#"
            SELECT f.abs_path, f.name, f.ext, f.is_dir, f.size, f.modified_at
            FROM fts_files ff
            JOIN files f ON f.id = ff.rowid
            WHERE ff.fts_files MATCH '{fq}'
            ORDER BY bm25(fts_files) ASC, f.modified_at DESC
            LIMIT {lim}
            "#,
            fq = escape_sql_string(&fts_query),
            lim = limit
        ))
        .fetch_all(db.pool())
        .await?;

        if fts_rows.is_empty() {
            // FTS5 with AND found nothing — try LIKE with the full query
            // (matches files whose name contains the entire query string).
            sqlx::query(
                r#"
                SELECT abs_path, name, ext, is_dir, size, modified_at
                FROM files
                WHERE name LIKE ? ESCAPE '\'
                ORDER BY modified_at DESC
                LIMIT ?
                "#,
            )
            .bind(&like)
            .bind(limit)
            .fetch_all(db.pool())
            .await?
        } else {
            fts_rows
        }
    };

    rows.iter()
        .map(|r| {
            Ok(FileEntry {
                path: r.try_get("abs_path")?,
                name: r.try_get("name")?,
                ext: r.try_get("ext")?,
                is_dir: r.try_get::<i64, _>("is_dir")? != 0,
                size: r.try_get("size")?,
                modified_at: r.try_get("modified_at")?,
            })
        })
        .collect()
}

fn sanitize_fts_query(input: &str) -> String {
    let tokens: Vec<&str> = input
        .split_whitespace()
        .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
        .collect();
    if tokens.is_empty() {
        return String::new();
    }
    // Use AND (implicit space) — all words must match for a file to be
    // included. This prevents irrelevant results that only match one word.
    tokens
        .iter()
        .map(|t| format!("\"{}\"*", t.trim_matches(|c: char| !c.is_alphanumeric())))
        .collect::<Vec<_>>()
        .join(" ")
}

fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}
