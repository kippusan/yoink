-- Junction table for many-to-many album ↔ artist relationships.
-- `ordering` controls display order; the lowest value is the primary artist.
CREATE TABLE album_artists (
    album_id    BLOB NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    artist_id   BLOB NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    ordering    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (album_id, artist_id)
);

CREATE INDEX idx_album_artists_artist ON album_artists(artist_id);

-- Seed from the existing albums.artist_id column so nothing is lost.
INSERT OR IGNORE INTO album_artists (album_id, artist_id, ordering)
SELECT id, artist_id, 0 FROM albums WHERE artist_id IS NOT NULL;
