-- ─────────────────────────────────────────────────────────────────────────────
-- Migration 0007: Add `custom_title` column to clipboard table.
-- Allows renaming pinned clipboard items for easier searching.
-- ─────────────────────────────────────────────────────────────────────────────

ALTER TABLE clipboard ADD COLUMN custom_title TEXT;
