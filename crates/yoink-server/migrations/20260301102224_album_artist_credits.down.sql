-- SQLite doesn't support DROP COLUMN before 3.35; this is best-effort.
ALTER TABLE albums DROP COLUMN artist_credits;
