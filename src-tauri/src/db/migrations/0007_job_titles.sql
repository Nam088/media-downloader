-- Display titles for the queue UI, which previously only had source_url to
-- show. `title` is this job's own display name (a video's title, or NULL for
-- rows created before this migration / paths where no title data was
-- available). `playlist_title` is shared across every job fanned out from
-- the same playlist submission (same value on every row with the same
-- parent_playlist_id), used as a group header instead of duplicating a
-- separate playlist table. Both plain nullable TEXT, no CHECK constraint, so
-- a simple ADD COLUMN is enough, same as 0006.
ALTER TABLE download_jobs ADD COLUMN title TEXT;
ALTER TABLE download_jobs ADD COLUMN playlist_title TEXT;
