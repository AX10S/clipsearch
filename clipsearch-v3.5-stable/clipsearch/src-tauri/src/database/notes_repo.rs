// ─────────────────────────────────────────────────────────────────────────────
// Notes repository — SQL queries for the `notes` + `fts_notes` tables.
//
// Like the clipboard repo, the FTS5 mirror is auto-synced by triggers.
// The repo exposes:
//   upsert     — insert or update a single note (called by the indexer)
//   delete_by_path
//   delete_by_vault   — full clear when the vault is switched
//   get_by_id
//   search     — FTS5 + exact/prefix ranking
//   count / count_vault
// ─────────────────────────────────────────────────────────────────────────────

use sqlx::sqlite::SqlitePool;
use sqlx::Row;

/// A note row, as returned to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Note {
    pub id: i64,
    pub vault_path: String,
    pub rel_path: String,
    pub abs_path: String,
    pub title: String,
    pub body: String,
    pub headings: String,
    pub aliases: String,
    pub tags: String,
    pub modified_at: i64,
    pub indexed_at: i64,
    pub pinned: bool,
}

/// Payload used when upserting a note.
#[derive(Debug, Clone)]
pub struct NotePayload {
    pub vault_path: String,
    pub rel_path: String,
    pub abs_path: String,
    pub title: String,
    pub body: String,
    pub headings: String,
    pub aliases: String,
    pub tags: String,
    pub modified_at: i64,
    pub indexed_at: i64,
}

/// Insert or update a note. Updates on `abs_path` conflict — i.e. the file was
/// edited and is being re-indexed. The FTS5 mirror is updated automatically by
/// the `notes_au` trigger.
pub async fn upsert(pool: &SqlitePool, note: NotePayload) -> anyhow::Result<i64> {
    let row = sqlx::query(
        r#"
        INSERT INTO notes
            (vault_path, rel_path, abs_path, title, body, headings, aliases, tags,
             modified_at, indexed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(abs_path) DO UPDATE SET
            vault_path   = excluded.vault_path,
            rel_path     = excluded.rel_path,
            title        = excluded.title,
            body         = excluded.body,
            headings     = excluded.headings,
            aliases      = excluded.aliases,
            tags         = excluded.tags,
            modified_at  = excluded.modified_at,
            indexed_at   = excluded.indexed_at
        RETURNING id
        "#,
    )
    .bind(&note.vault_path)
    .bind(&note.rel_path)
    .bind(&note.abs_path)
    .bind(&note.title)
    .bind(&note.body)
    .bind(&note.headings)
    .bind(&note.aliases)
    .bind(&note.tags)
    .bind(note.modified_at)
    .bind(note.indexed_at)
    .fetch_one(pool)
    .await?;

    Ok(row.try_get::<i64, _>("id")?)
}

/// Delete a note by its absolute path. Called when the file is removed/renamed.
pub async fn delete_by_path(pool: &SqlitePool, abs_path: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM notes WHERE abs_path = ?")
        .bind(abs_path)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete all notes belonging to a vault. Used when switching vaults.
pub async fn delete_by_vault(pool: &SqlitePool, vault_path: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM notes WHERE vault_path = ?")
        .bind(vault_path)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_by_id(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<Note>> {
    let row = sqlx::query(
        r#"
        SELECT id, vault_path, rel_path, abs_path, title, body, headings,
               aliases, tags, modified_at, indexed_at, pinned
        FROM notes WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.map(|r| row_to_note(&r)).transpose()
}

/// FTS5 search over notes with two ranking signals:
///   - exact title match  (rank boost)
///   - bm25(fts_notes)    (relevance)
/// `rel_path` is included in the FTS index so users can search by filename too.
pub async fn search(pool: &SqlitePool, query: &str, limit: i64) -> anyhow::Result<Vec<Note>> {
    if query.trim().is_empty() {
        return recent(pool, limit).await;
    }
    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        // Pure punctuation — fall back to a title LIKE scan.
        let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let rows = sqlx::query(
            r#"
            SELECT id, vault_path, rel_path, abs_path, title, body, headings,
                   aliases, tags, modified_at, indexed_at, pinned
            FROM notes
            WHERE title LIKE ? ESCAPE '\' OR rel_path LIKE ? ESCAPE '\'
            ORDER BY modified_at DESC
            LIMIT ?
            "#,
        )
        .bind(&like)
        .bind(&like)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        return rows.iter().map(row_to_note).collect();
    }

    // BM25 in SQLite FTS5 returns more-negative = better match. We order by
    // bm25 ascending (best first), then break ties by recency.
    let rows = sqlx::query(&format!(
        r#"
        SELECT n.id, n.vault_path, n.rel_path, n.abs_path, n.title, n.body,
               n.headings, n.aliases, n.tags, n.modified_at, n.indexed_at, n.pinned
        FROM fts_notes f
        JOIN notes n ON n.id = f.rowid
        WHERE f.fts_notes MATCH '{fq}'
        ORDER BY
            n.pinned DESC,
            (n.title = '{eq}') DESC,
            (n.title LIKE '{pp}' ESCAPE '\') DESC,
            bm25(fts_notes) ASC,
            n.modified_at DESC
        LIMIT {lim}
        "#,
        fq = escape_sql_string(&fts_query),
        eq = escape_sql_string(query),
        pp = escape_sql_string(&format!("{}%", query.replace('%', "\\%").replace('_', "\\_"))),
        lim = limit
    ))
    .fetch_all(pool)
    .await?;

    rows.iter().map(row_to_note).collect()
}

/// Most recently modified notes (used when the query is empty).
pub async fn recent(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<Note>> {
    let rows = sqlx::query(
        r#"
        SELECT id, vault_path, rel_path, abs_path, title, body, headings,
               aliases, tags, modified_at, indexed_at, pinned
        FROM notes
        ORDER BY pinned DESC, modified_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_to_note).collect()
}

pub async fn count(pool: &SqlitePool) -> anyhow::Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM notes")
        .fetch_one(pool)
        .await?;
    Ok(row.try_get::<i64, _>("n")?)
}

pub async fn count_vault(pool: &SqlitePool, vault_path: &str) -> anyhow::Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM notes WHERE vault_path = ?")
        .bind(vault_path)
        .fetch_one(pool)
        .await?;
    Ok(row.try_get::<i64, _>("n")?)
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn row_to_note(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Note> {
    Ok(Note {
        id: row.try_get("id")?,
        vault_path: row.try_get("vault_path")?,
        rel_path: row.try_get("rel_path")?,
        abs_path: row.try_get("abs_path")?,
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        headings: row.try_get("headings")?,
        aliases: row.try_get("aliases")?,
        tags: row.try_get("tags")?,
        modified_at: row.try_get("modified_at")?,
        indexed_at: row.try_get("indexed_at")?,
        pinned: row.try_get::<i64, _>("pinned")? != 0,
    })
}

/// Toggle the `pinned` flag of a note (favorites).
pub async fn set_pinned(pool: &SqlitePool, id: i64, pinned: bool) -> anyhow::Result<()> {
    sqlx::query("UPDATE notes SET pinned = ? WHERE id = ?")
        .bind(pinned)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Same logic as `clipboard_repo::sanitize_fts_query` — kept duplicated to
/// keep the two repos independent (clean architecture: no shared util import).
///
/// Strips leading `#` from hashtag-style queries (so `#tag` finds `tag` in
/// the `tags` column). Returns tokens joined with ` OR ` so that multi-word
/// queries match notes containing ANY of the words (more forgiving for the
/// user), and each token is quoted + prefixed with `*` for prefix matching.
fn sanitize_fts_query(input: &str) -> String {
    // Strip leading `#` from each token (hashtag syntax) so `#tag` → `tag`.
    let tokens: Vec<String> = input
        .split_whitespace()
        .map(|t| t.trim_start_matches('#').to_string())
        .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
        .map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return String::new();
    }
    // Use AND (implicit space) — all words must match for a note to be
    // included. This prevents irrelevant results that only match one word.
    tokens
        .iter()
        .map(|t| format!("\"{}\"*", t))
        .collect::<Vec<_>>()
        .join(" ")
}

fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_sanitize_cyrillic() {
        assert_eq!(
            sanitize_fts_query("привет мир"),
            "\"привет\"* OR \"мир\"*"
        );
    }

    #[test]
    fn fts_sanitize_strips_hashtag() {
        assert_eq!(sanitize_fts_query("#tag1"), "\"tag1\"*");
        assert_eq!(
            sanitize_fts_query("#foo #bar"),
            "\"foo\"* OR \"bar\"*"
        );
    }
}
