use chrono::{DateTime, Utc};
use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};
use tracing::info;

use crate::models::{DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist};

/// Open (or create) the database and run migrations.
pub(crate) async fn open(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect(url)
        .await?;

    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await?;

    migrate(&pool).await?;
    info!(url, "Database opened");
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS artists (
            id          INTEGER PRIMARY KEY,
            name        TEXT    NOT NULL,
            picture     TEXT,
            tidal_url   TEXT,
            quality_profile TEXT NOT NULL DEFAULT 'LOSSLESS',
            added_at    TEXT    NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS albums (
            id           INTEGER PRIMARY KEY,
            artist_id    INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
            title        TEXT    NOT NULL,
            album_type   TEXT,
            release_date TEXT,
            cover        TEXT,
            tidal_url    TEXT,
            explicit     INTEGER NOT NULL DEFAULT 0,
            monitored    INTEGER NOT NULL DEFAULT 0,
            acquired     INTEGER NOT NULL DEFAULT 0,
            wanted       INTEGER NOT NULL DEFAULT 0,
            added_at     TEXT    NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query("ALTER TABLE albums ADD COLUMN album_type TEXT")
        .execute(pool)
        .await
        .ok();

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS download_jobs (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            album_id         INTEGER NOT NULL,
            artist_id        INTEGER NOT NULL,
            album_title      TEXT    NOT NULL,
            status           TEXT    NOT NULL DEFAULT 'queued',
            quality          TEXT    NOT NULL DEFAULT 'LOSSLESS',
            total_tracks     INTEGER NOT NULL DEFAULT 0,
            completed_tracks INTEGER NOT NULL DEFAULT 0,
            error            TEXT,
            created_at       TEXT    NOT NULL,
            updated_at       TEXT    NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_albums_artist ON albums(artist_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_jobs_album ON download_jobs(album_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_jobs_status ON download_jobs(status)")
        .execute(pool)
        .await?;

    Ok(())
}

// ── Artists ─────────────────────────────────────────────────────────

pub(crate) async fn load_artists(pool: &SqlitePool) -> Result<Vec<MonitoredArtist>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, picture, tidal_url, quality_profile, added_at FROM artists ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| MonitoredArtist {
            id: r.get("id"),
            name: r.get("name"),
            picture: r.get("picture"),
            tidal_url: r.get("tidal_url"),
            quality_profile: r.get("quality_profile"),
            added_at: parse_dt(r.get::<String, _>("added_at")),
        })
        .collect())
}

pub(crate) async fn upsert_artist(pool: &SqlitePool, artist: &MonitoredArtist) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO artists (id, name, picture, tidal_url, quality_profile, added_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           picture = excluded.picture,
           tidal_url = excluded.tidal_url,
           quality_profile = excluded.quality_profile",
    )
    .bind(artist.id)
    .bind(&artist.name)
    .bind(&artist.picture)
    .bind(&artist.tidal_url)
    .bind(&artist.quality_profile)
    .bind(artist.added_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_artist(pool: &SqlitePool, artist_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM artists WHERE id = $1")
        .bind(artist_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Albums ──────────────────────────────────────────────────────────

pub(crate) async fn load_albums(pool: &SqlitePool) -> Result<Vec<MonitoredAlbum>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, artist_id, title, album_type, release_date, cover, tidal_url, explicit,
                monitored, acquired, wanted, added_at
         FROM albums ORDER BY release_date DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| MonitoredAlbum {
            id: r.get("id"),
            artist_id: r.get("artist_id"),
            title: r.get("title"),
            album_type: r.get("album_type"),
            release_date: r.get("release_date"),
            cover: r.get("cover"),
            tidal_url: r.get("tidal_url"),
            explicit: r.get::<i32, _>("explicit") != 0,
            monitored: r.get::<i32, _>("monitored") != 0,
            acquired: r.get::<i32, _>("acquired") != 0,
            wanted: r.get::<i32, _>("wanted") != 0,
            added_at: parse_dt(r.get::<String, _>("added_at")),
        })
        .collect())
}

pub(crate) async fn upsert_album(pool: &SqlitePool, album: &MonitoredAlbum) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO albums (id, artist_id, title, album_type, release_date, cover, tidal_url,
                             explicit, monitored, acquired, wanted, added_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         ON CONFLICT(id) DO UPDATE SET
           artist_id = excluded.artist_id,
           title = excluded.title,
           album_type = excluded.album_type,
           release_date = excluded.release_date,
           cover = excluded.cover,
           tidal_url = excluded.tidal_url,
           explicit = excluded.explicit,
           monitored = excluded.monitored,
           acquired = excluded.acquired,
           wanted = excluded.wanted",
    )
    .bind(album.id)
    .bind(album.artist_id)
    .bind(&album.title)
    .bind(&album.album_type)
    .bind(&album.release_date)
    .bind(&album.cover)
    .bind(&album.tidal_url)
    .bind(album.explicit as i32)
    .bind(album.monitored as i32)
    .bind(album.acquired as i32)
    .bind(album.wanted as i32)
    .bind(album.added_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_albums_by_artist(pool: &SqlitePool, artist_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM albums WHERE artist_id = $1")
        .bind(artist_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_album(pool: &SqlitePool, album_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM albums WHERE id = $1")
        .bind(album_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn update_album_flags(
    pool: &SqlitePool,
    album_id: i64,
    monitored: bool,
    acquired: bool,
    wanted: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE albums SET monitored = $1, acquired = $2, wanted = $3 WHERE id = $4")
        .bind(monitored as i32)
        .bind(acquired as i32)
        .bind(wanted as i32)
        .bind(album_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Download jobs ───────────────────────────────────────────────────

pub(crate) async fn load_jobs(pool: &SqlitePool) -> Result<Vec<DownloadJob>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, album_id, artist_id, album_title, status, quality,
                total_tracks, completed_tracks, error, created_at, updated_at
         FROM download_jobs ORDER BY id DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| DownloadJob {
            id: r.get::<i64, _>("id") as u64,
            album_id: r.get("album_id"),
            artist_id: r.get("artist_id"),
            album_title: r.get("album_title"),
            status: parse_status(&r.get::<String, _>("status")),
            quality: r.get("quality"),
            total_tracks: r.get::<i32, _>("total_tracks") as usize,
            completed_tracks: r.get::<i32, _>("completed_tracks") as usize,
            error: r.get("error"),
            created_at: parse_dt(r.get::<String, _>("created_at")),
            updated_at: parse_dt(r.get::<String, _>("updated_at")),
        })
        .collect())
}

pub(crate) async fn insert_job(pool: &SqlitePool, job: &DownloadJob) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO download_jobs (album_id, artist_id, album_title, status, quality,
                                    total_tracks, completed_tracks, error, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(job.album_id)
    .bind(job.artist_id)
    .bind(&job.album_title)
    .bind(job.status.as_str())
    .bind(&job.quality)
    .bind(job.total_tracks as i32)
    .bind(job.completed_tracks as i32)
    .bind(&job.error)
    .bind(job.created_at.to_rfc3339())
    .bind(job.updated_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub(crate) async fn update_job(pool: &SqlitePool, job: &DownloadJob) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE download_jobs SET status = $1, total_tracks = $2, completed_tracks = $3,
                                  error = $4, updated_at = $5
         WHERE id = $6",
    )
    .bind(job.status.as_str())
    .bind(job.total_tracks as i32)
    .bind(job.completed_tracks as i32)
    .bind(&job.error)
    .bind(job.updated_at.to_rfc3339())
    .bind(job.id as i64)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn delete_job(pool: &SqlitePool, job_id: u64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM download_jobs WHERE id = $1")
        .bind(job_id as i64)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_completed_jobs(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM download_jobs WHERE status = 'completed'")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ── Helpers ─────────────────────────────────────────────────────────

fn parse_dt(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_status(s: &str) -> DownloadStatus {
    match s {
        "queued" => DownloadStatus::Queued,
        "resolving" => DownloadStatus::Resolving,
        "downloading" => DownloadStatus::Downloading,
        "completed" => DownloadStatus::Completed,
        "failed" => DownloadStatus::Failed,
        _ => DownloadStatus::Failed,
    }
}
