-- ─────────────────────────────────────────────────────────────────────────────
-- Migration 0006: Add `custom_title` column to favorites table.
-- Allows the user to rename favorites for easier searching.
-- ─────────────────────────────────────────────────────────────────────────────

ALTER TABLE favorites ADD COLUMN custom_title TEXT;
