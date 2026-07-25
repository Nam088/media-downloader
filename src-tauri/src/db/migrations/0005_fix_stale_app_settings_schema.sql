-- Corrects a real migration mistake: 0004's SQL content was rewritten
-- (from an ALTER-TABLE-ADD-COLUMN version to the key-value version) after
-- some installs had already applied the old version — `rusqlite_migration`
-- tracks progress by count, not by content hash, so those installs recorded
-- "migration 4 applied" against SQL that no longer matches what's in
-- 0004_settings_key_value.sql today, leaving `app_settings` stuck on the
-- old rigid-column schema while the Rust code expects key/value columns
-- ("table app_settings has no column named key").
--
-- Unconditionally dropping and recreating is safe regardless of which
-- shape an install is actually in: installs already correctly on the
-- key-value schema just get an equivalent empty table back (Rust's
-- `get_setting_or_default` repopulates every key from its defaults on the
-- next read, same as any fresh install); installs stuck on the old schema
-- get corrected. Pre-release, no real user settings worth preserving over
-- either path.
DROP TABLE IF EXISTS app_settings;

CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
