-- ─────────────────────────────────────────────────────────────────────────────
-- Migration 0005: favorites table — for pinning any search result type.
--
-- Unlike clipboard/notes which have their own `pinned` column, file/app/web
-- results are ephemeral (not stored in their own tables). So we store their
-- "pinned" status here, keyed by a stable identifier (path, url, etc.).
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS favorites (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    source      TEXT NOT NULL,          -- 'file' | 'app' | 'web'
    key         TEXT NOT NULL,          -- path / exec_path / url
    title       TEXT NOT NULL,
    subtitle    TEXT,                    -- optional: e.g. file path, provider
    created_at  INTEGER NOT NULL,
    UNIQUE(source, key)
);

CREATE INDEX IF NOT EXISTS idx_fav_source ON favorites(source);
CREATE INDEX IF NOT EXISTS idx_fav_key    ON favorites(key);
