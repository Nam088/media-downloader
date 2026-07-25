-- Lets a gallery job download only a user-picked subset of the images
-- gallery-dl found, instead of always everything. Stored as a JSON array of
-- URL strings (NULL = no selection was made, i.e. "everything"). A plain
-- nullable TEXT column needs no CHECK constraint, so a simple ADD COLUMN is
-- enough here (unlike the gallery_mode/media_type migrations, no table
-- rebuild needed).
ALTER TABLE download_jobs ADD COLUMN selected_gallery_urls TEXT;
