CREATE TABLE artists (
    id          BLOB PRIMARY KEY,
    name        TEXT NOT NULL,
    image_url   TEXT,
    added_at    TEXT NOT NULL
);

CREATE TABLE albums (
    id              BLOB PRIMARY KEY,
    artist_id       BLOB NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    album_type      TEXT,
    release_date    TEXT,
    cover_url       TEXT,
    explicit        INTEGER NOT NULL DEFAULT 0,
    monitored       INTEGER NOT NULL DEFAULT 0,
    acquired        INTEGER NOT NULL DEFAULT 0,
    wanted          INTEGER NOT NULL DEFAULT 0,
    added_at        TEXT NOT NULL
);

CREATE TABLE tracks (
    id              BLOB PRIMARY KEY,
    album_id        BLOB NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    disc_number     INTEGER NOT NULL DEFAULT 1,
    track_number    INTEGER NOT NULL,
    duration_secs   INTEGER,
    explicit        INTEGER NOT NULL DEFAULT 0,
    isrc            TEXT
);

CREATE TABLE artist_provider_links (
    id              BLOB PRIMARY KEY,
    artist_id       BLOB NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    external_id     TEXT NOT NULL,
    external_url    TEXT,
    external_name   TEXT,
    image_ref       TEXT,
    UNIQUE(provider, external_id)
);

CREATE TABLE album_provider_links (
    id              BLOB PRIMARY KEY,
    album_id        BLOB NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    external_id     TEXT NOT NULL,
    external_url    TEXT,
    external_title  TEXT,
    cover_ref       TEXT,
    UNIQUE(provider, external_id)
);

CREATE TABLE track_provider_links (
    id              BLOB PRIMARY KEY,
    track_id        BLOB NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    external_id     TEXT NOT NULL,
    UNIQUE(provider, external_id)
);

CREATE TABLE download_jobs (
    id               BLOB PRIMARY KEY,
    album_id         BLOB NOT NULL REFERENCES albums(id),
    source           TEXT NOT NULL,
    album_title      TEXT NOT NULL,
    artist_name      TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'queued',
    quality          TEXT NOT NULL DEFAULT 'lossless',
    total_tracks     INTEGER NOT NULL DEFAULT 0,
    completed_tracks INTEGER NOT NULL DEFAULT 0,
    error            TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

CREATE INDEX idx_albums_artist ON albums(artist_id);
CREATE INDEX idx_tracks_album ON tracks(album_id);
CREATE INDEX idx_artist_links_artist ON artist_provider_links(artist_id);
CREATE INDEX idx_artist_links_provider ON artist_provider_links(provider, external_id);
CREATE INDEX idx_album_links_album ON album_provider_links(album_id);
CREATE INDEX idx_album_links_provider ON album_provider_links(provider, external_id);
CREATE INDEX idx_track_links_track ON track_provider_links(track_id);
CREATE INDEX idx_track_links_provider ON track_provider_links(provider, external_id);
CREATE INDEX idx_jobs_album ON download_jobs(album_id);
CREATE INDEX idx_jobs_status ON download_jobs(status);
CREATE INDEX idx_tracks_isrc ON tracks(isrc);
