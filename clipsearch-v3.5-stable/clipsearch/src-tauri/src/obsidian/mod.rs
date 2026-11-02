// ─────────────────────────────────────────────────────────────────────────────
// Obsidian integration.
//
// Per the spec: NO use of the Obsidian API. Instead, recursively index every
// Markdown file in a user-selected vault directory, extract metadata, and keep
// a SQLite FTS5 index in sync. The filesystem is watched via `notify`, and
// only changed files are re-indexed — never the whole vault.
//
// Searching never touches the filesystem; it queries SQLite only.
//
// Opening a note uses the `obsidian://open` URI scheme (or reveals in Finder
// / Explorer if Obsidian is unavailable).
//
// Module surface:
//   ObsidianIndexer::start(vault_path, db, app)  -> spawns background tasks
//   ObsidianIndexer::reindex_now()                -> force full re-scan
//   ObsidianIndexer::status()                     -> stats for the UI
//
// Parsing is deliberately non-pulldown-cmark — we don't need a full Markdown
// AST, just title, headings, aliases, tags. A line-oriented scan is ~10x
// faster and good enough.
// ─────────────────────────────────────────────────────────────────────────────

mod parser;
mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::database::notes_repo::{self, NotePayload};
use crate::database::Database;

/// Events emitted to the frontend during indexing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum IndexEvent {
    Started { total: usize },
    Progress { done: usize, total: usize },
    Finished { indexed: i64, took_ms: u64 },
    FileUpdated { rel_path: String },
    FileRemoved { rel_path: String },
    Error { message: String },
}

/// Runtime status of the indexer.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IndexerStatus {
    pub vault_path: Option<String>,
    pub indexed_count: i64,
    pub last_scan_ms: u64,
    pub scanning: bool,
}

/// Commands sent to the indexer worker.
enum Cmd {
    FullScan,
    UpsertFile(PathBuf),
    RemoveFile(PathBuf),
    Stop,
}

pub struct ObsidianIndexer {
    vault: PathBuf,
    tx: mpsc::Sender<Cmd>,
    status: Arc<Mutex<IndexerStatus>>,
}

impl ObsidianIndexer {
    /// Spawn the indexer. Performs an initial full scan in the background.
    pub fn start(vault: PathBuf, db: Arc<Database>, app: AppHandle) -> Result<Arc<Self>> {
        if !vault.is_dir() {
            anyhow::bail!("vault path is not a directory: {}", vault.display());
        }

        let status = Arc::new(Mutex::new(IndexerStatus {
            vault_path: Some(vault.display().to_string()),
            indexed_count: 0,
            last_scan_ms: 0,
            scanning: false,
        }));

        let (tx, rx) = mpsc::channel::<Cmd>(256);

        let indexer = Arc::new(Self {
            vault: vault.clone(),
            tx: tx.clone(),
            status: Arc::clone(&status),
        });

        // Spawn the worker task. It owns the receiver.
        let vault_for_worker = vault.clone();
        let status_for_worker = Arc::clone(&status);
        tauri::async_runtime::spawn(async move {
            worker(vault_for_worker, db, app, status_for_worker, rx).await;
        });

        // Spawn the filesystem watcher.
        watcher::spawn(vault.clone(), tx.clone())?;

        // Kick off an initial scan.
        let _ = tx.try_send(Cmd::FullScan);

        tracing::info!(vault = ?vault, "obsidian indexer started");
        Ok(indexer)
    }

    /// Force a full re-scan.
    pub fn reindex_now(&self) {
        let _ = self.tx.try_send(Cmd::FullScan);
    }

    /// Snapshot of the current status.
    pub fn status(&self) -> IndexerStatus {
        self.status.lock().clone()
    }

    /// Resolve a note's absolute path from its relative path within the vault.
    pub fn resolve_abs(&self, rel_path: &str) -> PathBuf {
        self.vault.join(rel_path)
    }
}

/// The worker loop. Receives commands and performs scans / single-file updates.
async fn worker(
    vault: PathBuf,
    db: Arc<Database>,
    app: AppHandle,
    status: Arc<Mutex<IndexerStatus>>,
    mut rx: mpsc::Receiver<Cmd>,
) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Stop => break,
            Cmd::FullScan => {
                full_scan(&vault, &db, &app, &status).await;
            }
            Cmd::UpsertFile(abs_path) => {
                if let Err(e) = upsert_file(&vault, &db, &abs_path).await {
                    tracing::warn!(error = ?e, path = ?abs_path, "upsert_file failed");
                }
                let rel = rel_path(&vault, &abs_path);
                let _ = app.emit(
                    "obsidian://event",
                    IndexEvent::FileUpdated { rel_path: rel },
                );
                refresh_count(&db, &status).await;
            }
            Cmd::RemoveFile(abs_path) => {
                let rel = rel_path(&vault, &abs_path);
                if let Err(e) = notes_repo::delete_by_path(db.pool(), &abs_path.to_string_lossy())
                    .await
                {
                    tracing::warn!(error = ?e, path = ?abs_path, "delete_by_path failed");
                }
                let _ = app.emit(
                    "obsidian://event",
                    IndexEvent::FileRemoved { rel_path: rel },
                );
                refresh_count(&db, &status).await;
            }
        }
    }
}

/// Walk the vault, find all `*.md` files, and upsert each into the DB.
/// Files that no longer exist are pruned afterwards.
async fn full_scan(
    vault: &Path,
    db: &Database,
    app: &AppHandle,
    status: &Arc<Mutex<IndexerStatus>>,
) {
    {
        let mut s = status.lock();
        if s.scanning {
            return; // already running
        }
        s.scanning = true;
    }

    let started = std::time::Instant::now();
    let files = discover_markdown(vault);
    let total = files.len();
    let _ = app.emit("obsidian://event", IndexEvent::Started { total });
    tracing::info!(total, "starting full vault scan");

    let mut done = 0usize;
    for chunk in files.chunks(50) {
        for f in chunk {
            if let Err(e) = upsert_file(vault, db, f).await {
                tracing::warn!(error = ?e, path = ?f, "upsert failed during scan");
            }
            done += 1;
        }
        let _ = app.emit(
            "obsidian://event",
            IndexEvent::Progress { done, total },
        );
        // Yield between batches so the search path is never starved on huge vaults.
        tokio::task::yield_now().await;
    }

    prune_deleted(vault, db).await;
    refresh_count(db, status).await;

    let took_ms = started.elapsed().as_millis() as u64;
    let indexed = status.lock().indexed_count;

    {
        let mut s = status.lock();
        s.scanning = false;
        s.last_scan_ms = took_ms;
    }

    let _ = app.emit(
        "obsidian://event",
        IndexEvent::Finished { indexed, took_ms },
    );
    tracing::info!(indexed, took_ms, "vault scan complete");
}

/// Recursively list every `.md` file under `vault`, skipping `.obsidian/` and
/// `.trash/` directories (Obsidian internal dirs).
fn discover_markdown(vault: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(vault, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip Obsidian's internal directories + node_modules.
        if path.is_dir() {
            match name_str.as_ref() {
                ".obsidian" | ".trash" | "node_modules" => continue,
                _ => walk(&path, out),
            }
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        } else if path.extension().and_then(|e| e.to_str()) == Some("canvas") {
            // Obsidian Canvas files — JSON with text content in `text` fields.
            out.push(path);
        }
    }
}

/// Read + parse + upsert a single Markdown or Canvas file.
async fn upsert_file(vault: &Path, db: &Database, abs_path: &Path) -> Result<()> {
    let bytes = std::fs::read(abs_path)
        .with_context(|| format!("reading {}", abs_path.display()))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();

    let metadata = std::fs::metadata(abs_path)?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    let rel = rel_path(vault, abs_path);

    // Dispatch to the right parser based on file extension.
    let parsed = if abs_path.extension().and_then(|e| e.to_str()) == Some("canvas") {
        parser::parse_canvas(&text, &rel)
    } else {
        parser::parse(&text, &rel)
    };

    let payload = NotePayload {
        vault_path: vault.display().to_string(),
        rel_path: rel,
        abs_path: abs_path.display().to_string(),
        title: parsed.title,
        body: parsed.body,
        headings: parsed.headings.join("\n"),
        aliases: parsed.aliases.join("\n"),
        tags: parsed.tags.join("\n"),
        modified_at,
        indexed_at: chrono::Utc::now().timestamp_millis(),
    };

    notes_repo::upsert(db.pool(), payload).await?;
    Ok(())
}

/// Remove DB rows whose `abs_path` no longer exists on disk.
async fn prune_deleted(vault: &Path, db: &Database) {
    let rows: Vec<String> = match sqlx::query_scalar::<_, String>(
        "SELECT abs_path FROM notes WHERE vault_path = ?",
    )
    .bind(vault.display().to_string())
    .fetch_all(db.pool())
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = ?e, "prune_deleted: fetch failed");
            return;
        }
    };

    for abs in rows {
        if !Path::new(&abs).exists() {
            let _ = notes_repo::delete_by_path(db.pool(), &abs).await;
        }
    }
}

/// Refresh `indexed_count` from the DB. Cheap (single COUNT(*)) and runs in
/// the worker task so it doesn't block search.
async fn refresh_count(db: &Database, status: &Arc<Mutex<IndexerStatus>>) {
    let count = notes_repo::count(db.pool()).await.unwrap_or(0);
    status.lock().indexed_count = count;
}

fn rel_path(vault: &Path, abs: &Path) -> String {
    abs.strip_prefix(vault)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().into_owned())
}
