-- ─────────────────────────────────────────────────────────────────────────────
-- Migration 0002: Add `pinned` column to notes table (for favorites).
--
-- Pinned notes always appear at the top of search results, similar to how
-- pinned clipboard items work. Default is 0 (not pinned).
-- ─────────────────────────────────────────────────────────────────────────────

ALTER TABLE notes ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_notes_pinned ON notes(pinned, modified_at DESC);
