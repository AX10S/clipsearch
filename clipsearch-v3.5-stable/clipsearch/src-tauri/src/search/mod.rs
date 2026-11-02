// ─────────────────────────────────────────────────────────────────────────────
// Search engine — the heart of the launcher.
//
// Searches clipboard + Obsidian notes simultaneously and merges them into one
// ranked list. The merge happens in Rust (not SQL) so we can apply the
// cross-source ranking weights described in the spec:
//
//   1. Exact title match      (highest weight — what the user typed IS a title)
//   2. Prefix match           (title starts with the query)
//   3. FTS5 BM25 score        (relevance, normalized per source)
//   4. Clipboard recency      (fresher = better)
//   5. Note recency           (recently modified = better)
//
// All queries hit SQLite only — no filesystem access, no API calls. Each
// query arm runs in its own `tokio::spawn` so they execute concurrently.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use serde::Serialize;

use crate::database::clipboard_repo::{self, ClipboardItem};
use crate::database::notes_repo::{self, Note};
use crate::database::Database;

/// One unified result row, sent to the frontend.
///
/// Five sources:
///   - clipboard  — text from the clipboard history
///   - note       — Obsidian Markdown / Canvas note
///   - file       — file or directory on the local filesystem
///   - app        — installed application
///   - web        — web search suggestion
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SearchResult {
    Clipboard {
        id: i64,
        title: String,
        preview: String,
        /// Raw content of the clipboard item — used by the frontend to:
        ///   • detect URLs (show Link icon) for `kind = "text"`
        ///   • parse file paths & extensions for `kind = "files"`
        content: String,
        /// "text" | "image" | "files" — drives the icon choice in the frontend.
        kind: String,
        source_app_name: Option<String>,
        created_at: i64,
        last_used_at: i64,
        pinned: bool,
        image_path: Option<String>,
        custom_title: Option<String>,
        score: f64,
    },
    Note {
        id: i64,
        title: String,
        preview: String,
        rel_path: String,
        vault_path: String,
        tags: Vec<String>,
        modified_at: i64,
        pinned: bool,
        score: f64,
    },
    File {
        path: String,
        name: String,
        ext: String,
        is_dir: bool,
        size: i64,
        modified_at: i64,
        score: f64,
    },
    // App variant removed — apps source is no longer used.
    // Keeping App variant in enum for backwards compat with existing DB entries.
    App {
        name: String,
        exec_path: String,
        bundle_id: Option<String>,
        version: Option<String>,
        icon_path: Option<String>,
        score: f64,
    },
    Web {
        query: String,
        provider: String,
        url: String,
        display_text: String,
        score: f64,
    },
    Itsm {
        query: String,
        url: String,
        score: f64,
    },
    OneNote {
        path: String,
        name: String,
        modified_at: i64,
        score: f64,
    },
    Confl {
        query: String,
        url: String,
        score: f64,
    },
    Jira {
        query: String,
        url: String,
        score: f64,
    },
}

impl SearchResult {
    pub fn id_str(&self) -> String {
        match self {
            SearchResult::Clipboard { id, .. } => format!("clip:{}", id),
            SearchResult::Note { id, .. } => format!("note:{}", id),
            SearchResult::File { path, .. } => format!("file:{}", path),
            SearchResult::App { exec_path, .. } => format!("app:{}", exec_path),
            SearchResult::Web { url, .. } => format!("web:{}", url),
            SearchResult::Itsm { url, .. } => format!("itsm:{}", url),
            SearchResult::OneNote { path, .. } => format!("onenote:{}", path),
            SearchResult::Confl { url, .. } => format!("confl:{}", url),
            SearchResult::Jira { url, .. } => format!("jira:{}", url),
        }
    }

    pub fn title(&self) -> &str {
        match self {
            SearchResult::Clipboard { title, .. } => title,
            SearchResult::Note { title, .. } => title,
            SearchResult::File { name, .. } => name,
            SearchResult::App { name, .. } => name,
            SearchResult::Web { display_text, .. } => display_text,
            SearchResult::Itsm { query, .. } => query,
            SearchResult::OneNote { name, .. } => name,
            SearchResult::Confl { query, .. } => query,
            SearchResult::Jira { query, .. } => query,
        }
    }

    pub fn preview(&self) -> &str {
        match self {
            SearchResult::Clipboard { preview, .. } => preview,
            SearchResult::Note { preview, .. } => preview,
            SearchResult::File { path, .. } => path,
            SearchResult::App { exec_path, .. } => exec_path,
            SearchResult::Web { url, .. } => url,
            SearchResult::Itsm { url, .. } => url,
            SearchResult::OneNote { path, .. } => path,
            SearchResult::Confl { url, .. } => url,
            SearchResult::Jira { url, .. } => url,
        }
    }
}

pub struct SearchEngine {
    db: Arc<Database>,
    web_provider: parking_lot::RwLock<String>,
    enabled_sources: parking_lot::RwLock<EnabledSources>,
}

/// Which sources are enabled in settings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnabledSources {
    pub clipboard: bool,
    pub obsidian: bool,
    pub files: bool,
    pub web: bool,
    pub itsm: bool,
    pub onenote: bool,
    pub confl: bool,
    pub jira: bool,
}

impl Default for EnabledSources {
    fn default() -> Self {
        Self {
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
}

impl SearchEngine {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            web_provider: parking_lot::RwLock::new("google".to_string()),
            enabled_sources: parking_lot::RwLock::new(EnabledSources::default()),
        }
    }

    pub fn set_web_provider(&self, provider: &str) {
        *self.web_provider.write() = provider.to_string();
    }

    pub fn set_enabled_sources(&self, sources: EnabledSources) {
        *self.enabled_sources.write() = sources;
    }

    /// Run a unified search across all enabled sources. Returns up to `limit`
    /// results, ranked and merged.
    ///
    /// To avoid flooding the list with one source (e.g. 50 files when the
    /// query matches many filenames), each source is capped to `per_source_limit`
    /// results before merging.
    pub async fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        let q = query.trim();
        let _inner_limit = (limit * 3).max(50) as i64;
        // Cap each source so no single source dominates the list.
        let per_source_limit = (limit.max(10)) as i64;
        let sources = self.enabled_sources.read().clone();
        let now_ms = chrono::Utc::now().timestamp_millis();

        // ── Launch all enabled source queries concurrently ────────────────────
        let clip_fut = async {
            if sources.clipboard {
                clipboard_repo::search(self.db.pool(), q, per_source_limit).await.ok()
            } else {
                None
            }
        };
        let note_fut = async {
            if sources.obsidian {
                notes_repo::search(self.db.pool(), q, per_source_limit).await.ok()
            } else {
                None
            }
        };
        let file_fut = async {
            if sources.files {
                crate::files::search(&self.db, q, per_source_limit).await.ok()
            } else {
                None
            }
        };
        let apps_fut = async { None::<Vec<()>> };
        // Clone the web provider string BEFORE the async block so we don't
        // hold a `RwLockReadGuard` across `.await` — parking_lot guards are
        // not `Send`, and Tauri commands require `Send` futures.
        let web_provider = self.web_provider.read().clone();
        let web_fut = async {
            if sources.web {
                Some(crate::web::search(q, &web_provider, (limit * 2).max(5)).await)
            } else {
                None
            }
        };

        // ITSM — only included when query is non-empty. Generates a single
        // result row that opens the Naumen ITSM portal in the browser.
        let itsm_fut = async {
            if sources.itsm && !q.is_empty() {
                Some(crate::web::build_itsm_url(q))
            } else {
                None
            }
        };

        // Confluence (Sberworks) — single result row opening a Confluence
        // search in the browser.
        let confl_fut = async {
            if sources.confl && !q.is_empty() {
                Some(crate::web::build_confluence_url(q))
            } else {
                None
            }
        };

        // Jira (Sberworks) — single result row opening a Jira JQL search.
        let jira_fut = async {
            if sources.jira && !q.is_empty() {
                Some(crate::web::build_jira_url(q))
            } else {
                None
            }
        };

        let (clip_res, note_res, file_res, _apps_res, web_res, itsm_res, confl_res, jira_res) =
            tokio::join!(clip_fut, note_fut, file_fut, apps_fut, web_fut, itsm_fut, confl_fut, jira_fut);

        let clips = clip_res.unwrap_or_default();
        let notes = note_res.unwrap_or_default();
        let files = file_res.unwrap_or_default();
        let web = web_res.unwrap_or_default();

        let mut scored: Vec<(f64, SearchResult)> = Vec::with_capacity(
            clips.len() + notes.len() + files.len() + web.len(),
        );

        // ── Score clipboard items ──────────────────────────────────────────────
        for c in clips {
            let s = score_clipboard(&c, q, now_ms);
            let preview = make_preview(&c.content, q);

            // Backfill `image_path` for OLD clipboard entries that were copied
            // BEFORE the watcher learned to set image_path for image files.
            // If kind == "files" and image_path is None, check whether the
            // first file path looks like an image — if so, expose that path.
            let image_path = c.image_path.clone().or_else(|| {
                if c.kind == "files" {
                    c.content.lines().next().and_then(|first_path| {
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

            scored.push((
                s,
                SearchResult::Clipboard {
                    id: c.id,
                    // Title = custom_title if set, else first NON-EMPTY line
                    // of content. Using `lines().next()` was buggy — if
                    // content starts with "\n", title would be empty.
                    title: c.custom_title.clone().unwrap_or_else(|| {
                        c.content
                            .lines()
                            .find(|line| !line.trim().is_empty())
                            .unwrap_or("")
                            .to_string()
                    }),
                    preview,
                    content: c.content.clone(),
                    kind: c.kind.clone(),
                    source_app_name: c.source_app_name,
                    created_at: c.created_at,
                    last_used_at: c.last_used_at,
                    pinned: c.pinned,
                    image_path,
                    custom_title: c.custom_title,
                    score: s,
                },
            ));
        }

        // ── Score notes ────────────────────────────────────────────────────────
        for n in notes {
            let s = score_note(&n, q, now_ms);
            let preview = make_preview(&n.body, q);
            scored.push((
                s,
                SearchResult::Note {
                    id: n.id,
                    title: n.title,
                    preview,
                    rel_path: n.rel_path,
                    vault_path: n.vault_path,
                    tags: if n.tags.is_empty() {
                        Vec::new()
                    } else {
                        n.tags.split('\n').map(|s| s.to_string()).collect()
                    },
                    modified_at: n.modified_at,
                    pinned: n.pinned,
                    score: s,
                },
            ));
        }

        // ── Score files (and OneNote .one files) ───────────────────────────────
        for f in files {
            let s = score_file(&f, q, now_ms);

            // OneNote .one files get their own source type.
            if f.ext.eq_ignore_ascii_case("one") {
                scored.push((
                    s,
                    SearchResult::OneNote {
                        path: f.path.clone(),
                        name: f.name.clone(),
                        modified_at: f.modified_at,
                        score: s,
                    },
                ));
            } else {
                scored.push((
                    s,
                    SearchResult::File {
                        path: f.path,
                        name: f.name,
                        ext: f.ext,
                        is_dir: f.is_dir,
                        size: f.size,
                        modified_at: f.modified_at,
                        score: s,
                    },
                ));
            }
        }

        // Apps source removed — no scoring needed.

        // ── Score web (lowest priority — only when query is non-empty) ─────────
        if !q.is_empty() {
            for (i, w) in web.into_iter().enumerate() {
                // Web results get a small score; the first one (the "search on"
                // row) gets a slightly higher score than suggestions.
                let s = if i == 0 { 5.0 } else { 3.0 };
                scored.push((
                    s,
                    SearchResult::Web {
                        query: w.query,
                        provider: w.provider,
                        url: w.url,
                        display_text: w.display_text,
                        score: s,
                    },
                ));
            }
        }

        // ── Score ITSM (same priority as web — single row) ───────────────────
        if let Some(itsm_url) = itsm_res {
            scored.push((
                5.0,
                SearchResult::Itsm {
                    query: q.to_string(),
                    url: itsm_url,
                    score: 5.0,
                },
            ));
        }

        // ── Score Confluence (Sberworks) — single row ────────────────────────
        if let Some(confl_url) = confl_res {
            scored.push((
                5.0,
                SearchResult::Confl {
                    query: q.to_string(),
                    url: confl_url,
                    score: 5.0,
                },
            ));
        }

        // ── Score Jira (Sberworks) — single row ───────────────────────────────
        if let Some(jira_url) = jira_res {
            scored.push((
                5.0,
                SearchResult::Jira {
                    query: q.to_string(),
                    url: jira_url,
                    score: 5.0,
                },
            ));
        }

        // Sort by score descending; stable sort preserves insertion order
        // within ties (which preserves per-source recency order).
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        Ok(scored.into_iter().map(|(_, r)| r).collect())
    }
}

// ── Per-source scoring ───────────────────────────────────────────────────────

fn score_file(f: &crate::files::FileEntry, query: &str, now_ms: i64) -> f64 {
    if query.is_empty() {
        return recency_bonus(f.modified_at, now_ms, W_RECENT_NOTE);
    }
    let mut score = 0.0;
    let name_lower = f.name.to_lowercase();
    let query_lower = query.to_lowercase();
    if f.name == query {
        score += W_EXACT;
    } else if name_lower.starts_with(&query_lower) {
        score += W_PREFIX;
    } else if name_lower.contains(&query_lower) {
        score += W_PREFIX * 0.4;
    }
    // Directories get a small boost (users often want to navigate to folders).
    if f.is_dir {
        score += 20.0;
    }
    // Only give recency bonus if the file actually matched the query.
    // Files from FTS5/LIKE that don't match the query text would still
    // appear with a small score — don't inflate them with recency.
    if score > 0.0 {
        score += recency_bonus(f.modified_at, now_ms, W_RECENT_NOTE);
    }
    score
}

// Apps source removed — score_app deleted.
fn _score_app_removed() {}

// ── Scoring ──────────────────────────────────────────────────────────────────
//
// Weights are tuned so that:
//   • Exact title match always wins (weight 1000)
//   • Prefix match is next (weight 500)
//   • BM25 contributes a small bonus (max ~50)
//   • Recency contributes a small bonus (max ~50, decays over 7 days)
//   • Pinned clipboard items get +200 (the user explicitly kept them)

const W_EXACT: f64 = 1000.0;
const W_PREFIX: f64 = 500.0;
const W_PINNED: f64 = 200.0;
const W_RECENT_CLIP: f64 = 50.0;
const W_RECENT_NOTE: f64 = 50.0;
const W_BM25_BONUS: f64 = 50.0;

fn score_clipboard(c: &ClipboardItem, query: &str, now_ms: i64) -> f64 {
    if query.is_empty() {
        // Empty query: pure recency ranking — NO pinned bonus. This keeps
        // favorited items in their natural position; favoriting a row
        // should only add a star icon, not move the row to the top.
        return recency_bonus(c.last_used_at, now_ms, W_RECENT_CLIP);
    }

    let first_line = c.content.lines().next().unwrap_or("");
    let mut score = 0.0;

    if first_line == query {
        score += W_EXACT;
    } else if first_line.to_lowercase().starts_with(&query.to_lowercase()) {
        score += W_PREFIX;
    } else if first_line.to_lowercase().contains(&query.to_lowercase()) {
        score += W_PREFIX * 0.3;
    }

    score += recency_bonus(c.last_used_at, now_ms, W_RECENT_CLIP);
    // NOTE: NO W_PINNED bonus — see the empty-query branch comment.
    score
}

fn score_note(n: &Note, query: &str, now_ms: i64) -> f64 {
    if query.is_empty() {
        // Empty query: pure recency — NO pinned bonus (same rationale as
        // score_clipboard).
        return recency_bonus(n.modified_at, now_ms, W_RECENT_NOTE);
    }

    let mut score = 0.0;
    let title_lower = n.title.to_lowercase();
    let query_lower = query.to_lowercase();

    if n.title == query {
        score += W_EXACT;
    } else if title_lower.starts_with(&query_lower) {
        score += W_PREFIX;
    } else if title_lower.contains(&query_lower) {
        score += W_PREFIX * 0.4;
    }

    if !title_lower.contains(&query_lower) {
        score += W_BM25_BONUS * 0.5;
    }

    score += recency_bonus(n.modified_at, now_ms, W_RECENT_NOTE);
    // NOTE: NO W_PINNED bonus — see score_clipboard.
    score
}

/// Recency bonus that decays linearly over `decay_ms`, but never reaches
/// exactly 0 — there's always a small floor (1.0) so old items still show up
/// in empty-query results instead of being silently dropped.
fn recency_bonus(ts_ms: i64, now_ms: i64, max_bonus: f64) -> f64 {
    let age_ms = (now_ms - ts_ms).max(0) as f64;
    let decay_ms = 30.0 * 24.0 * 60.0 * 60.0 * 1000.0; // 30 days (was 7)
    if age_ms >= decay_ms {
        // Floor: very old items still get a tiny bonus so they appear in the
        // list (otherwise the launcher would look empty when the user has
        // mostly old notes).
        return 1.0;
    }
    // Linear decay from max_bonus (now) down to 1.0 (at decay_ms).
    1.0 + (max_bonus - 1.0) * (1.0 - age_ms / decay_ms)
}

/// Build a single-line preview around the first occurrence of `query`.
fn make_preview(content: &str, query: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let first_line = content.lines().next().unwrap_or("");
    if query.is_empty() {
        return truncate(first_line, 160);
    }

    // Find the query (case-insensitive) in the whole content; fall back to
    // the first line if not found.
    let lower = content.to_lowercase();
    let ql = query.to_lowercase();
    if let Some(pos) = lower.find(&ql) {
        // Expand a window of ~80 chars around the match.
        // Use a safe boundary alignment to avoid splitting multi-byte UTF-8
        // sequences (e.g. Cyrillic characters are 2 bytes each).
        let raw_start = pos.saturating_sub(40);
        let raw_end = (pos + ql.len() + 80).min(content.len());
        let start = align_to_char_boundary(content, raw_start, false);
        let end = align_to_char_boundary(content, raw_end, true);
        let window = &content[start..end];
        let prefix = if start > 0 { "…" } else { "" };
        let suffix = if end < content.len() { "…" } else { "" };
        let preview = format!("{}{}{}", prefix, window.replace('\n', " "), suffix);
        return truncate(&preview, 180);
    }
    truncate(first_line, 160)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{}…", truncated)
}

/// Align a byte index to a UTF-8 char boundary.
/// `ceil = false` → move backwards to the nearest char start (floor).
/// `ceil = true`  → move forwards to the nearest char start (ceil).
fn align_to_char_boundary(s: &str, idx: usize, ceil: bool) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    if s.is_char_boundary(idx) {
        return idx;
    }
    if ceil {
        // Move forward to the next char boundary.
        let mut i = idx;
        while i < s.len() && !s.is_char_boundary(i) {
            i += 1;
        }
        i
    } else {
        // Move backward to the previous char boundary.
        let mut i = idx;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recency_decays_over_seven_days() {
        let now = 1_700_000_000_000i64;
        let fresh = recency_bonus(now, now, 50.0);
        assert!((fresh - 50.0).abs() < 0.01);

        // One week ago: still within the 30-day decay window, so non-zero.
        let week_ago = now - (7 * 24 * 60 * 60 * 1000);
        let v_week = recency_bonus(week_ago, now, 50.0);
        assert!(v_week > 1.0 && v_week < 50.0);

        // 30 days ago: at the edge of decay — should return the floor (1.0).
        let month_ago = now - (30 * 24 * 60 * 60 * 1000);
        let v_month = recency_bonus(month_ago, now, 50.0);
        assert!((v_month - 1.0).abs() < 0.01);

        // Very old: floor (1.0).
        let year_ago = now - (365 * 24 * 60 * 60 * 1000);
        let v_year = recency_bonus(year_ago, now, 50.0);
        assert!((v_year - 1.0).abs() < 0.01);

        let half = now - (15 * 24 * 60 * 60 * 1000);
        let v = recency_bonus(half, now, 50.0);
        assert!(v > 1.0 && v < 50.0);
    }

    #[test]
    fn preview_finds_query_in_body() {
        let s = "title line\nthis is the **matching** text we want to show\nmore";
        let p = make_preview(s, "matching");
        assert!(p.contains("matching"));
        assert!(p.contains("…") || p.starts_with("title"));
    }

    #[test]
    fn preview_truncates_long_lines() {
        let long = "x".repeat(500);
        let p = make_preview(&long, "");
        assert!(p.chars().count() <= 161); // 160 + ellipsis
    }
}
