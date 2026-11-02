// ─────────────────────────────────────────────────────────────────────────────
// Database module — SQLite via sqlx, with WAL mode and FTS5.
//
// Public surface:
//   Database::open(path) -> Arc<Database>        (async)
//   Database::pool()     -> &SqlitePool          (shared with the search engine)
//
// The pool is sized at 8 connections, enough for concurrent search + clipboard
// writes + obsidian indexing without contention. WAL mode allows readers to
// never block writers — critical for the "typing never freezes" requirement.
// ─────────────────────────────────────────────────────────────────────────────

pub mod clipboard_repo;
pub mod notes_repo;

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

/// Thin wrapper around a `SqlitePool`. Held in `Arc` and shared across modules.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Open (or create) the database at `path`, apply pragmas and run migrations.
    ///
    /// This is async because sqlx's connection + migration APIs are async. The
    /// caller (Tauri's synchronous `setup` hook) wraps it in
    /// `tauri::async_runtime::block_on`.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 database path"))?;

        // ── Connection options: WAL + tuned pragmas ──────────────────────────
        // These are applied to EVERY connection the pool creates — important
        // because pragmas are per-connection in SQLite.
        let options = SqliteConnectOptions::from_str(conn_str)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .pragma("mmap_size", "268435456") // 256 MB memory-mapped IO
            .pragma("temp_store", "MEMORY")
            .pragma("cache_size", "-65536") // 64 MB page cache
            .pragma("foreign_keys", "ON")
            .pragma("busy_timeout", "5000") // 5 s lock-acquisition timeout
            .pragma("wal_autocheckpoint", "1000");

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .min_connections(2) // keep 2 warm connections for the search path
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect_with(options)
            .await?;

        sqlx::query("PRAGMA optimize").execute(&pool).await.ok();

        tracing::info!(path = conn_str, "database opened");

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    /// Run embedded migrations. The `sqlx::migrate!` macro bakes them into the
    /// binary at compile time — no external files needed at runtime.
    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("./src/database/migrations")
            .run(&self.pool)
            .await
            .context("failed to run migrations")?;
        tracing::info!("migrations applied");
        Ok(())
    }

    /// Borrow the underlying pool. Used by repositories and the search engine.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
