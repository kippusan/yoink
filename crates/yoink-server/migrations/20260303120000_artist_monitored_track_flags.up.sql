-- Add monitored flag to artists (existing artists are fully monitored)
ALTER TABLE artists ADD COLUMN monitored INTEGER NOT NULL DEFAULT 1;

-- Add track-level monitoring and acquired flags
ALTER TABLE tracks ADD COLUMN monitored INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tracks ADD COLUMN acquired INTEGER NOT NULL DEFAULT 0;

-- Index for finding wanted tracks (monitored but not acquired)
CREATE INDEX idx_tracks_monitored ON tracks(monitored);
