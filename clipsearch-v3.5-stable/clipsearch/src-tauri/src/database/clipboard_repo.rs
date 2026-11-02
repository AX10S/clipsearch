// ─────────────────────────────────────────────────────────────────────────────
// Clipboard repository — SQL queries for the `clipboard` + `fts_clipboard` tables.
//
// All functions are async and take `&SqlitePool`. The FTS5 mirror is kept in
// sync automatically by triggers defined in `migrations/0001_init.sql`, so the
// repository code only ever touches the base `clipboard` table.
// ─────────────────────────────────────────────────────────────────────────────

use sqlx::sqlite::SqlitePool;
use sqlx::Row;

/// One clipboard entry, as returned to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClipboardItem {
    pub id: i64,
    pub content: String,
    pub kind: String,
    pub byte_size: i64,
    pub source_app: Option<String>,
    pub source_app_name: Option<String>,
    pub created_at: i64,
    pub last_used_at: i64,
    pub pinned: bool,
    pub use_count: i64,
    pub image_path: Option<String>,
    pub custom_title: Option<String>,
}

/// Payload used when inserting a new clipboard capture.
#[derive(Debug, Clone)]
pub struct NewClipboardItem {
    pub content: String,
    pub content_hash: String,
    pub kind: String,
    pub byte_size: i64,
    pub source_app: Option<String>,
    pub source_app_name: Option<String>,
    pub now_ms: i64,
    pub image_path: Option<String>,
}

/// Insert a clipboard item, OR — if the hash already exists — bump its
/// `last_used_at` and `use_count`. This is the dedup path: only one row per
/// unique content, regardless of how many times it was copied.
///
/// Returns the row id (existing or newly inserted).
pub async fn upsert(pool: &SqlitePool, item: NewClipboardItem) -> anyhow::Result<i64> {
    let row = sqlx::query(
        r#"
        INSERT INTO clipboard
            (content, content_hash, kind, byte_size, source_app, source_app_name,
             created_at, last_used_at, pinned, use_count, image_path)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 1, ?)
        ON CONFLICT(content_hash) DO UPDATE SET
            last_used_at = excluded.last_used_at,
            use_count    = clipboard.use_count + 1
        RETURNING id
        "#,
    )
    .bind(&item.content)
    .bind(&item.content_hash)
    .bind(&item.kind)
    .bind(item.byte_size)
    .bind(&item.source_app)
    .bind(&item.source_app_name)
    .bind(item.now_ms)
    .bind(item.now_ms)
    .bind(&item.image_path)
    .fetch_one(pool)
    .await?;

    Ok(row.try_get::<i64, _>("id")?)
}

/// Touch `last_used_at` when an existing item is re-copied from the UI.
pub async fn touch(pool: &SqlitePool, id: i64, now_ms: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE clipboard SET last_used_at = ?, use_count = use_count + 1 WHERE id = ?")
        .bind(now_ms)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch the most recent N items, pinned first then by recency.
pub async fn recent(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<ClipboardItem>> {
    let rows = sqlx::query(
        r#"
        SELECT id, content, kind, byte_size, source_app, source_app_name,
               created_at, last_used_at, pinned, use_count, image_path, custom_title
        FROM clipboard
        ORDER BY pinned DESC, last_used_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.iter().map(row_to_item).collect()
}

/// FTS5 + LIKE search over clipboard content. Returns up to `limit` items.
///
/// The query has two arms joined with `UNION ALL`:
///   1. Exact / prefix matches on `content` (high signal, always shown first).
///   2. FTS5 BM25 matches via `fts_clipboard` (fuzzy, ranked by relevance).
///
/// `UNION ALL` (not `UNION`) avoids a duplicate-removal sort — we dedup in
/// Rust by id afterwards.
pub async fn search(pool: &SqlitePool, query: &str, limit: i64) -> anyhow::Result<Vec<ClipboardItem>> {
    if query.trim().is_empty() {
        return recent(pool, limit).await;
    }

    let like_pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
    let prefix_pattern = format!("{}%", query.replace('%', "\\%").replace('_', "\\_"));
    let fts_query = sanitize_fts_query(query);

    let sql = if fts_query.is_empty() {
        // No usable FTS tokens (e.g. user typed only punctuation). Fall back to
        // a pure LIKE scan — still indexed via the rowid + hash index.
        r#"
        SELECT id, content, kind, byte_size, source_app, source_app_name,
               created_at, last_used_at, pinned, use_count, image_path, custom_title
        FROM clipboard
        WHERE content LIKE ? ESCAPE '\'
        ORDER BY pinned DESC,
                 (content = ?) DESC,
                 (content LIKE ? ESCAPE '\') DESC,
                 last_used_at DESC
        LIMIT ?
        "#
        .to_string()
    } else {
        format!(
            r#"
            WITH exact AS (
                SELECT id, content, kind, byte_size, source_app, source_app_name,
                       created_at, last_used_at, pinned, use_count, image_path, custom_title,
                       CASE WHEN content = '{eq}' THEN 1 ELSE 0 END AS is_exact
                FROM clipboard
                WHERE content LIKE '{lp}' ESCAPE '\'
            ),
            fts AS (
                SELECT c.id, c.content, c.kind, c.byte_size, c.source_app, c.source_app_name,
                       c.created_at, c.last_used_at, c.pinned, c.use_count, c.image_path, c.custom_title,
                       0 AS is_exact
                FROM fts_clipboard f
                JOIN clipboard c ON c.id = f.rowid
                WHERE f.fts_clipboard MATCH '{fq}'
                ORDER BY bm25(fts_clipboard)
                LIMIT {lim}
            )
            SELECT * FROM exact
            UNION ALL
            SELECT * FROM fts WHERE id NOT IN (SELECT id FROM exact)
            ORDER BY is_exact DESC, pinned DESC, last_used_at DESC
            LIMIT {lim}
            "#,
            eq = escape_sql_string(query),
            lp = escape_sql_string(&like_pattern),
            fq = escape_sql_string(&fts_query),
            lim = limit
        )
    };

    // Bind the LIKE pattern separately to avoid SQL-injection pitfalls on the
    // LIKE arm (the FTS arm uses an escaped literal).
    let rows = if fts_query.is_empty() {
        sqlx::query(&sql)
            .bind(&like_pattern)
            .bind(query)
            .bind(&prefix_pattern)
            .bind(limit)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query(&sql).fetch_all(pool).await?
    };

    // Dedup by id (UNION ALL can theoretically yield the same row twice if
    // an exact match is also a strong FTS hit).
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows.iter() {
        let id: i64 = row.try_get("id")?;
        if seen.insert(id) {
            out.push(row_to_item(row)?);
        }
    }
    Ok(out)
}

/// Toggle the `pinned` flag of an item.
pub async fn set_pinned(pool: &SqlitePool, id: i64, pinned: bool) -> anyhow::Result<()> {
    sqlx::query("UPDATE clipboard SET pinned = ? WHERE id = ?")
        .bind(pinned)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a single item by id.
pub async fn delete(pool: &SqlitePool, id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM clipboard WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update the content of a clipboard item. Used by the "Edit" feature in the
/// preview pane — the user can fix typos or trim text before re-copying.
/// Recomputes `content_hash` and `byte_size` automatically.
pub async fn update_content(
    pool: &SqlitePool,
    id: i64,
    new_content: String,
) -> anyhow::Result<()> {
    let hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(new_content.as_bytes());
        let bytes = hasher.finalize();
        let mut s = String::with_capacity(16);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for &b in &bytes[..8] {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0x0f) as usize] as char);
        }
        s
    };
    let byte_size = new_content.len() as i64;
    sqlx::query(
        r#"UPDATE clipboard
           SET content = ?, content_hash = ?, byte_size = ?
           WHERE id = ?"#,
    )
    .bind(&new_content)
    .bind(&hash)
    .bind(byte_size)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete all non-pinned items. Returns the number of deleted rows.
pub async fn clear_unpinned(pool: &SqlitePool) -> anyhow::Result<u64> {
    let res = sqlx::query("DELETE FROM clipboard WHERE pinned = 0")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Enforce the configured history cap. Called after every insert.
/// Pinned items are never evicted.
pub async fn enforce_limit(pool: &SqlitePool, limit: i64) -> anyhow::Result<()> {
    if limit <= 0 {
        return Ok(());
    }
    // Keep: (a) all pinned items, plus (b) the N most recent non-pinned items.
    // Everything else gets deleted. We compute "to keep" as a UNION of the two
    // sets, then delete the complement.
    sqlx::query(
        r#"
        DELETE FROM clipboard
        WHERE pinned = 0
          AND id NOT IN (
              SELECT id FROM clipboard
              WHERE pinned = 0
              ORDER BY last_used_at DESC
              LIMIT ?
          )
        "#,
    )
    .bind(limit)
    .execute(pool)
    .await?;
    Ok(())
}

/// Current count of items (used for the status panel).
pub async fn count(pool: &SqlitePool) -> anyhow::Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM clipboard")
        .fetch_one(pool)
        .await?;
    Ok(row.try_get::<i64, _>("n")?)
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn row_to_item(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<ClipboardItem> {
    Ok(ClipboardItem {
        id: row.try_get("id")?,
        content: row.try_get("content")?,
        kind: row.try_get("kind")?,
        byte_size: row.try_get("byte_size")?,
        source_app: row.try_get("source_app")?,
        source_app_name: row.try_get("source_app_name")?,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
        pinned: row.try_get::<i64, _>("pinned")? != 0,
        use_count: row.try_get("use_count")?,
        image_path: row.try_get("image_path")?,
        custom_title: row.try_get("custom_title")?,
    })
}

/// Update the custom title of a clipboard item (for pinned/favorited items).
pub async fn update_custom_title(
    pool: &SqlitePool,
    id: i64,
    custom_title: Option<String>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE clipboard SET custom_title = ? WHERE id = ?")
        .bind(&custom_title)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// FTS5 query strings have a special syntax (`*`, `"`, `AND`, `OR`, `NEAR`).
/// We sanitize by quoting tokens — turning `hello world` into `"hello" "world"`
/// — so the user can type arbitrary text without surprises. Pure punctuation
/// queries become empty (handled by the caller).
fn sanitize_fts_query(input: &str) -> String {
    let tokens: Vec<&str> = input
        .split_whitespace()
        .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
        .collect();
    if tokens.is_empty() {
        return String::new();
    }
    tokens
        .iter()
        .map(|t| format!("\"{}\"*", t.trim_matches(|c: char| !c.is_alphanumeric())))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escape single quotes for use as a SQL string literal.
fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_sanitize_simple() {
        assert_eq!(sanitize_fts_query("hello world"), "\"hello\"* \"world\"*");
    }

    #[test]
    fn fts_sanitize_punctuation_only() {
        assert_eq!(sanitize_fts_query("!@#$"), "");
    }

    #[test]
    fn fts_sanitize_strips_punct_around_tokens() {
        assert_eq!(sanitize_fts_query("(hello) [world]"), "\"hello\"* \"world\"*");
    }
}
