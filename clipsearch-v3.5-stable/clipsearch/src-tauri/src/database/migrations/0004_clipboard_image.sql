-- ─────────────────────────────────────────────────────────────────────────────
-- Migration 0004: Add `image_path` column to clipboard table.
--
-- When the user copies an image (not text), we save the image to disk and
-- store the path here. `content` is set to a placeholder like "[Image]"
-- so FTS5 still has something to index.
-- ─────────────────────────────────────────────────────────────────────────────

ALTER TABLE clipboard ADD COLUMN image_path TEXT;
