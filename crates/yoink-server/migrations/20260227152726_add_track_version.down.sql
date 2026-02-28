-- SQLite does not support DROP COLUMN before 3.35.0.
-- Recreate the table without the `version` column.

CREATE TABLE tracks_tmp (
    id              BLOB PRIMARY KEY,
    album_id        BLOB NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    disc_number     INTEGER NOT NULL DEFAULT 1,
    track_number    INTEGER NOT NULL,
    duration_secs   INTEGER,
    explicit        INTEGER NOT NULL DEFAULT 0,
    isrc            TEXT
);

INSERT INTO tracks_tmp (id, album_id, title, disc_number, track_number, duration_secs, explicit, isrc)
SELECT id, album_id, title, disc_number, track_number, duration_secs, explicit, isrc
FROM tracks;

DROP TABLE tracks;
ALTER TABLE tracks_tmp RENAME TO tracks;

CREATE INDEX idx_tracks_album ON tracks(album_id);
CREATE INDEX idx_tracks_isrc ON tracks(isrc);
