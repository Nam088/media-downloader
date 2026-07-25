-- Replaces the rigid single-row `app_settings` table (fixed columns, one
-- schema migration required per new setting — three of the last four
-- migrations in this project were exactly that) with a generic key-value
-- table of the same name. New settings from here on just need Rust-side
-- default handling, never another ALTER TABLE / table-rebuild migration.
-- Pre-release, no real user data to carry over — dropped rather than migrated.

DROP TABLE app_settings;

CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
