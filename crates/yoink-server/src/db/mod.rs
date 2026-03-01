mod album_artists;
mod albums;
mod artists;
mod jobs;
mod match_suggestions;
mod provider_links;
mod tracks;

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tracing::info;

pub(crate) use album_artists::{
    add_album_artist, delete_album_artists_by_artist, remove_album_artist, set_album_artists,
};
pub(crate) use albums::{
    delete_album, delete_albums_by_artist, load_albums, reassign_jobs_to_album,
    reassign_tracks_to_album, update_album_flags, upsert_album,
};
pub(crate) use artists::{
    delete_artist, load_artists, update_artist_bio, update_artist_details, upsert_artist,
};
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
