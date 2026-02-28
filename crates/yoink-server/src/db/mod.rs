mod albums;
mod artists;
mod jobs;
mod match_suggestions;
mod provider_links;
mod tracks;

use chrono::{DateTime, Utc};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tracing::info;
use uuid::Uuid;

pub(crate) use albums::{
    delete_album, delete_albums_by_artist, load_albums, reassign_jobs_to_album,
    reassign_tracks_to_album, update_album_flags, upsert_album,
};
pub(crate) use artists::{delete_artist, load_artists, update_artist_bio, upsert_artist};
pub(crate) use jobs::{delete_completed_jobs, delete_job, insert_job, load_jobs, update_job};
pub(crate) use match_suggestions::{
    MatchSuggestion, clear_pending_match_suggestions, load_match_suggestion_by_id,
    load_match_suggestions_for_scope, set_match_suggestion_status, upsert_match_suggestion,
};
pub(crate) use provider_links::{
    AlbumProviderLink, ArtistProviderLink, delete_artist_provider_link,
    find_album_by_provider_link, find_artist_by_provider_link, load_album_provider_links,
    load_artist_provider_links, upsert_album_provider_link, upsert_artist_provider_link,
    upsert_track_provider_link,
};
pub(crate) use tracks::{
    find_track_by_album_isrc, find_track_by_album_position, find_track_by_provider_link,
    load_tracks_for_album, upsert_track,
};

/// Generate a new UUID v7 (time-ordered).
pub(crate) fn new_uuid() -> Uuid {
    Uuid::now_v7()
}

/// Convert a UUID to its hyphenated string form (for JSON transport).
pub(crate) fn uuid_to_string(id: &Uuid) -> String {
    id.to_string()
}

/// Parse a UUID from its hyphenated string form.
pub(crate) fn parse_uuid(s: &str) -> Result<Uuid, String> {
    Uuid::parse_str(s).map_err(|e| format!("invalid UUID: {e}"))
}

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

    sqlx::migrate!("./migrations").run(&pool).await?;
    info!(url, "Database opened");
    Ok(pool)
}

// ── Shared helpers ──────────────────────────────────────────────────

fn parse_dt(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_status(s: &str) -> crate::models::DownloadStatus {
    use crate::models::DownloadStatus;
    match s {
        "queued" => DownloadStatus::Queued,
        "resolving" => DownloadStatus::Resolving,
        "downloading" => DownloadStatus::Downloading,
        "completed" => DownloadStatus::Completed,
        "failed" => DownloadStatus::Failed,
        _ => DownloadStatus::Failed,
    }
}
