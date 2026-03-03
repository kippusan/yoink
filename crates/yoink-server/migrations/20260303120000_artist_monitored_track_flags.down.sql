DROP INDEX IF EXISTS idx_tracks_monitored;

-- SQLite doesn't support DROP COLUMN in older versions, but modern SQLite (3.35+) does.
ALTER TABLE tracks DROP COLUMN acquired;
ALTER TABLE tracks DROP COLUMN monitored;
ALTER TABLE artists DROP COLUMN monitored;
