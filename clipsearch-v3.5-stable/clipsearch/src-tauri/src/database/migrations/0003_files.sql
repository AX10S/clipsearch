-- ─────────────────────────────────────────────────────────────────────────────
-- Migration 0003: files table — indexed filesystem for fast file search.
--
-- Only files/dirs under user-configured `indexed_folders` are stored here.
-- The list is kept in sync by the `notify` watcher in `files/mod.rs`.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS files (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    abs_path     TEXT    NOT NULL UNIQUE,
    name         TEXT    NOT NULL,
    ext          TEXT    NOT NULL DEFAULT '',
    is_dir       INTEGER NOT NULL DEFAULT 0,
    size         INTEGER NOT NULL DEFAULT 0,
    modified_at  INTEGER NOT NULL,
    indexed_at   INTEGER NOT NULL,
    root_folder  TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_files_name       ON files(name);
CREATE INDEX IF NOT EXISTS idx_files_ext        ON files(ext);
CREATE INDEX IF NOT EXISTS idx_files_modified   ON files(modified_at DESC);
CREATE INDEX IF NOT EXISTS idx_files_root       ON files(root_folder);

CREATE VIRTUAL TABLE IF NOT EXISTS fts_files USING fts5(
    name,
    content='files',
    content_rowid='id',
    tokenize='porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
    INSERT INTO fts_files(rowid, name) VALUES (new.id, new.name);
END;
CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
    INSERT INTO fts_files(fts_files, rowid, name) VALUES('delete', old.id, old.name);
END;
CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
    INSERT INTO fts_files(fts_files, rowid, name) VALUES('delete', old.id, old.name);
    INSERT INTO fts_files(rowid, name) VALUES (new.id, new.name);
END;
