-- ─────────────────────────────────────────────────────────────────────────────
-- ClipVault schema v1
--
-- All timestamps are stored as Unix milliseconds (i64).
-- `content_hash` is a hex SHA-256 of the underlying bytes — used for dedup.
-- FTS5 tables are kept in sync via triggers so the application code never has
-- to remember to update both sides.
-- ─────────────────────────────────────────────────────────────────────────────

-- ── Clipboard history (deduplicated) ────────────────────────────────────────
CREATE TABLE IF NOT EXISTS clipboard (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    content         TEXT    NOT NULL,
    content_hash    TEXT    NOT NULL UNIQUE,
    kind            TEXT    NOT NULL,                -- 'text' | 'image' | 'html' | 'rtf'
    byte_size       INTEGER NOT NULL,
    source_app      TEXT,                             -- bundle_id / process path
    source_app_name TEXT,                             -- friendly display name
    created_at      INTEGER NOT NULL,
    last_used_at    INTEGER NOT NULL,
    pinned          INTEGER NOT NULL DEFAULT 0,
    use_count       INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_clip_created   ON clipboard(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_clip_last_used ON clipboard(last_used_at DESC);
CREATE INDEX IF NOT EXISTS idx_clip_pinned    ON clipboard(pinned, last_used_at DESC);

-- FTS5 mirror of clipboard text content.
-- `unicode61 remove_diacritics 2` handles Latin + Cyrillic + accents well.
CREATE VIRTUAL TABLE IF NOT EXISTS fts_clipboard USING fts5(
    content,
    content='clipboard',
    content_rowid='id',
    tokenize='porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS clipboard_ai AFTER INSERT ON clipboard BEGIN
    INSERT INTO fts_clipboard(rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER IF NOT EXISTS clipboard_ad AFTER DELETE ON clipboard BEGIN
    INSERT INTO fts_clipboard(fts_clipboard, rowid, content) VALUES('delete', old.id, old.content);
END;
CREATE TRIGGER IF NOT EXISTS clipboard_au AFTER UPDATE ON clipboard BEGIN
    INSERT INTO fts_clipboard(fts_clipboard, rowid, content) VALUES('delete', old.id, old.content);
    INSERT INTO fts_clipboard(rowid, content) VALUES (new.id, new.content);
END;

-- ── Obsidian notes ──────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS notes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    vault_path   TEXT    NOT NULL,
    rel_path     TEXT    NOT NULL,
    abs_path     TEXT    NOT NULL UNIQUE,
    title        TEXT    NOT NULL,
    body         TEXT    NOT NULL,
    headings     TEXT    NOT NULL DEFAULT '',
    aliases      TEXT    NOT NULL DEFAULT '',
    tags         TEXT    NOT NULL DEFAULT '',
    modified_at  INTEGER NOT NULL,
    indexed_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notes_abs_path  ON notes(abs_path);
CREATE INDEX IF NOT EXISTS idx_notes_vault     ON notes(vault_path);
CREATE INDEX IF NOT EXISTS idx_notes_modified  ON notes(modified_at DESC);
CREATE INDEX IF NOT EXISTS idx_notes_title     ON notes(title);

CREATE VIRTUAL TABLE IF NOT EXISTS fts_notes USING fts5(
    title,
    body,
    headings,
    aliases,
    tags,
    rel_path,
    content='notes',
    content_rowid='id',
    tokenize='porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes BEGIN
    INSERT INTO fts_notes(rowid, title, body, headings, aliases, tags, rel_path)
    VALUES (new.id, new.title, new.body, new.headings, new.aliases, new.tags, new.rel_path);
END;
CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
    INSERT INTO fts_notes(fts_notes, rowid, title, body, headings, aliases, tags, rel_path)
    VALUES('delete', old.id, old.title, old.body, old.headings, old.aliases, old.tags, old.rel_path);
END;
CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
    INSERT INTO fts_notes(fts_notes, rowid, title, body, headings, aliases, tags, rel_path)
    VALUES('delete', old.id, old.title, old.body, old.headings, old.aliases, old.tags, old.rel_path);
    INSERT INTO fts_notes(rowid, title, body, headings, aliases, tags, rel_path)
    VALUES (new.id, new.title, new.body, new.headings, new.aliases, new.tags, new.rel_path);
END;

-- ── Schema version marker (for future hand-written migrations) ──────────────
CREATE TABLE IF NOT EXISTS schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO schema_meta(key, value) VALUES('schema_version', '1');
