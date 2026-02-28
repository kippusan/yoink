use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use tracing::info;
use uuid::Uuid;

use crate::models::{DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist, TrackInfo};

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

// ── Artists ─────────────────────────────────────────────────────────

pub(crate) async fn load_artists(pool: &SqlitePool) -> Result<Vec<MonitoredArtist>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, image_url, added_at FROM artists ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            MonitoredArtist {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                name: r.get("name"),
                image_url: r.get("image_url"),
                added_at: parse_dt(r.get::<String, _>("added_at")),
            }
        })
        .collect())
}

pub(crate) async fn upsert_artist(
    pool: &SqlitePool,
    artist: &MonitoredArtist,
) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(&artist.id).unwrap_or_else(|_| new_uuid());
    sqlx::query(
        "INSERT INTO artists (id, name, image_url, added_at)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           image_url = excluded.image_url",
    )
    .bind(uuid.as_bytes().as_slice())
    .bind(&artist.name)
    .bind(&artist.image_url)
    .bind(artist.added_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_artist(pool: &SqlitePool, artist_id: &str) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(artist_id).unwrap_or_default();
    sqlx::query("DELETE FROM artists WHERE id = $1")
        .bind(uuid.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

// ── Albums ──────────────────────────────────────────────────────────

pub(crate) async fn load_albums(pool: &SqlitePool) -> Result<Vec<MonitoredAlbum>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, artist_id, title, album_type, release_date, cover_url, explicit,
                monitored, acquired, wanted, added_at
         FROM albums ORDER BY release_date DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let artist_id: Vec<u8> = r.get("artist_id");
            MonitoredAlbum {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                artist_id: Uuid::from_slice(&artist_id).unwrap_or_default().to_string(),
                title: r.get("title"),
                album_type: r.get("album_type"),
                release_date: r.get("release_date"),
                cover_url: r.get("cover_url"),
                explicit: r.get::<i32, _>("explicit") != 0,
                monitored: r.get::<i32, _>("monitored") != 0,
                acquired: r.get::<i32, _>("acquired") != 0,
                wanted: r.get::<i32, _>("wanted") != 0,
                added_at: parse_dt(r.get::<String, _>("added_at")),
            }
        })
        .collect())
}

pub(crate) async fn upsert_album(
    pool: &SqlitePool,
    album: &MonitoredAlbum,
) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(&album.id).unwrap_or_else(|_| new_uuid());
    let artist_uuid = parse_uuid(&album.artist_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO albums (id, artist_id, title, album_type, release_date, cover_url,
                             explicit, monitored, acquired, wanted, added_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT(id) DO UPDATE SET
           artist_id = excluded.artist_id,
           title = excluded.title,
           album_type = excluded.album_type,
           release_date = excluded.release_date,
           cover_url = excluded.cover_url,
           explicit = excluded.explicit,
           monitored = excluded.monitored,
           acquired = excluded.acquired,
           wanted = excluded.wanted",
    )
    .bind(uuid.as_bytes().as_slice())
    .bind(artist_uuid.as_bytes().as_slice())
    .bind(&album.title)
    .bind(&album.album_type)
    .bind(&album.release_date)
    .bind(&album.cover_url)
    .bind(album.explicit as i32)
    .bind(album.monitored as i32)
    .bind(album.acquired as i32)
    .bind(album.wanted as i32)
    .bind(album.added_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_albums_by_artist(
    pool: &SqlitePool,
    artist_id: &str,
) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(artist_id).unwrap_or_default();
    sqlx::query("DELETE FROM albums WHERE artist_id = $1")
        .bind(uuid.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_album(pool: &SqlitePool, album_id: &str) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(album_id).unwrap_or_default();
    sqlx::query("DELETE FROM albums WHERE id = $1")
        .bind(uuid.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn update_album_flags(
    pool: &SqlitePool,
    album_id: &str,
    monitored: bool,
    acquired: bool,
    wanted: bool,
) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(album_id).unwrap_or_default();
    sqlx::query("UPDATE albums SET monitored = $1, acquired = $2, wanted = $3 WHERE id = $4")
        .bind(monitored as i32)
        .bind(acquired as i32)
        .bind(wanted as i32)
        .bind(uuid.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn reassign_tracks_to_album(
    pool: &SqlitePool,
    from_album_id: &str,
    to_album_id: &str,
) -> Result<u64, sqlx::Error> {
    let from_uuid = parse_uuid(from_album_id).unwrap_or_default();
    let to_uuid = parse_uuid(to_album_id).unwrap_or_default();
    let result = sqlx::query("UPDATE tracks SET album_id = $1 WHERE album_id = $2")
        .bind(to_uuid.as_bytes().as_slice())
        .bind(from_uuid.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn reassign_jobs_to_album(
    pool: &SqlitePool,
    from_album_id: &str,
    to_album_id: &str,
) -> Result<u64, sqlx::Error> {
    let from_uuid = parse_uuid(from_album_id).unwrap_or_default();
    let to_uuid = parse_uuid(to_album_id).unwrap_or_default();
    let result = sqlx::query("UPDATE download_jobs SET album_id = $1 WHERE album_id = $2")
        .bind(to_uuid.as_bytes().as_slice())
        .bind(from_uuid.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ── Tracks ──────────────────────────────────────────────────────────

pub(crate) async fn load_tracks_for_album(
    pool: &SqlitePool,
    album_id: &str,
) -> Result<Vec<TrackInfo>, sqlx::Error> {
    let uuid = parse_uuid(album_id).unwrap_or_default();
    let rows = sqlx::query(
        "SELECT id, title, version, disc_number, track_number, duration_secs, explicit, isrc
         FROM tracks WHERE album_id = $1
         ORDER BY disc_number, track_number",
    )
    .bind(uuid.as_bytes().as_slice())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let secs: Option<i32> = r.get("duration_secs");
            let secs = secs.unwrap_or(0) as u32;
            let mins = secs / 60;
            let rem = secs % 60;
            TrackInfo {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                title: r.get("title"),
                version: r.get("version"),
                disc_number: r.get::<i32, _>("disc_number") as u32,
                track_number: r.get::<i32, _>("track_number") as u32,
                duration_secs: secs,
                duration_display: format!("{mins}:{rem:02}"),
                isrc: r.get("isrc"),
                explicit: r.get::<i32, _>("explicit") != 0,
            }
        })
        .collect())
}

pub(crate) async fn upsert_track(
    pool: &SqlitePool,
    track: &TrackInfo,
    album_id: &str,
) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(&track.id).unwrap_or_else(|_| new_uuid());
    let album_uuid = parse_uuid(album_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO tracks (id, album_id, title, version, disc_number, track_number, duration_secs, explicit, isrc)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           version = excluded.version,
           disc_number = excluded.disc_number,
           track_number = excluded.track_number,
           duration_secs = excluded.duration_secs,
           explicit = excluded.explicit,
           isrc = excluded.isrc",
    )
    .bind(uuid.as_bytes().as_slice())
    .bind(album_uuid.as_bytes().as_slice())
    .bind(&track.title)
    .bind(&track.version)
    .bind(track.disc_number as i32)
    .bind(track.track_number as i32)
    .bind(track.duration_secs as i32)
    .bind(track.explicit as i32)
    .bind(&track.isrc)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn find_track_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT track_id FROM track_provider_links WHERE provider = $1 AND external_id = $2",
    )
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("track_id");
        Uuid::from_slice(&id).unwrap_or_default().to_string()
    }))
}

pub(crate) async fn find_track_by_album_isrc(
    pool: &SqlitePool,
    album_id: &str,
    isrc: &str,
) -> Result<Option<String>, sqlx::Error> {
    let album_uuid = parse_uuid(album_id).unwrap_or_default();
    let row = sqlx::query(
        "SELECT id FROM tracks WHERE album_id = $1 AND UPPER(isrc) = UPPER($2) LIMIT 1",
    )
    .bind(album_uuid.as_bytes().as_slice())
    .bind(isrc)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("id");
        Uuid::from_slice(&id).unwrap_or_default().to_string()
    }))
}

pub(crate) async fn find_track_by_album_position(
    pool: &SqlitePool,
    album_id: &str,
    disc_number: u32,
    track_number: u32,
) -> Result<Option<String>, sqlx::Error> {
    let album_uuid = parse_uuid(album_id).unwrap_or_default();
    let row = sqlx::query(
        "SELECT id
         FROM tracks
         WHERE album_id = $1 AND disc_number = $2 AND track_number = $3
         LIMIT 1",
    )
    .bind(album_uuid.as_bytes().as_slice())
    .bind(disc_number as i32)
    .bind(track_number as i32)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("id");
        Uuid::from_slice(&id).unwrap_or_default().to_string()
    }))
}

// ── Provider links ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ArtistProviderLink {
    pub(crate) id: String,
    pub(crate) artist_id: String,
    pub(crate) provider: String,
    pub(crate) external_id: String,
    pub(crate) external_url: Option<String>,
    pub(crate) external_name: Option<String>,
    pub(crate) image_ref: Option<String>,
}

pub(crate) async fn upsert_artist_provider_link(
    pool: &SqlitePool,
    link: &ArtistProviderLink,
) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(&link.id).unwrap_or_else(|_| new_uuid());
    let artist_uuid = parse_uuid(&link.artist_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO artist_provider_links (id, artist_id, provider, external_id, external_url, external_name, image_ref)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           external_url = excluded.external_url,
           external_name = excluded.external_name,
           image_ref = excluded.image_ref",
    )
    .bind(uuid.as_bytes().as_slice())
    .bind(artist_uuid.as_bytes().as_slice())
    .bind(&link.provider)
    .bind(&link.external_id)
    .bind(&link.external_url)
    .bind(&link.external_name)
    .bind(&link.image_ref)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_artist_provider_link(
    pool: &SqlitePool,
    artist_id: &str,
    provider: &str,
    external_id: &str,
) -> Result<(), sqlx::Error> {
    let artist_uuid = parse_uuid(artist_id).unwrap_or_default();
    sqlx::query(
        "DELETE FROM artist_provider_links WHERE artist_id = $1 AND provider = $2 AND external_id = $3",
    )
    .bind(artist_uuid.as_bytes().as_slice())
    .bind(provider)
    .bind(external_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_artist_provider_links(
    pool: &SqlitePool,
    artist_id: &str,
) -> Result<Vec<ArtistProviderLink>, sqlx::Error> {
    let uuid = parse_uuid(artist_id).unwrap_or_default();
    let rows = sqlx::query(
        "SELECT id, artist_id, provider, external_id, external_url, external_name, image_ref
         FROM artist_provider_links WHERE artist_id = $1",
    )
    .bind(uuid.as_bytes().as_slice())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let artist_id: Vec<u8> = r.get("artist_id");
            ArtistProviderLink {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                artist_id: Uuid::from_slice(&artist_id).unwrap_or_default().to_string(),
                provider: r.get("provider"),
                external_id: r.get("external_id"),
                external_url: r.get("external_url"),
                external_name: r.get("external_name"),
                image_ref: r.get("image_ref"),
            }
        })
        .collect())
}

/// Find a local artist by a provider link's external_id.
pub(crate) async fn find_artist_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT artist_id FROM artist_provider_links WHERE provider = $1 AND external_id = $2",
    )
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("artist_id");
        Uuid::from_slice(&id).unwrap_or_default().to_string()
    }))
}

#[derive(Debug, Clone)]
pub(crate) struct AlbumProviderLink {
    pub(crate) id: String,
    pub(crate) album_id: String,
    pub(crate) provider: String,
    pub(crate) external_id: String,
    pub(crate) external_url: Option<String>,
    pub(crate) external_title: Option<String>,
    pub(crate) cover_ref: Option<String>,
}

pub(crate) async fn upsert_album_provider_link(
    pool: &SqlitePool,
    link: &AlbumProviderLink,
) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(&link.id).unwrap_or_else(|_| new_uuid());
    let album_uuid = parse_uuid(&link.album_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO album_provider_links (id, album_id, provider, external_id, external_url, external_title, cover_ref)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           album_id = excluded.album_id,
           external_url = excluded.external_url,
           external_title = excluded.external_title,
           cover_ref = excluded.cover_ref",
    )
    .bind(uuid.as_bytes().as_slice())
    .bind(album_uuid.as_bytes().as_slice())
    .bind(&link.provider)
    .bind(&link.external_id)
    .bind(&link.external_url)
    .bind(&link.external_title)
    .bind(&link.cover_ref)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_album_provider_links(
    pool: &SqlitePool,
    album_id: &str,
) -> Result<Vec<AlbumProviderLink>, sqlx::Error> {
    let uuid = parse_uuid(album_id).unwrap_or_default();
    let rows = sqlx::query(
        "SELECT id, album_id, provider, external_id, external_url, external_title, cover_ref
         FROM album_provider_links WHERE album_id = $1",
    )
    .bind(uuid.as_bytes().as_slice())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let album_id: Vec<u8> = r.get("album_id");
            AlbumProviderLink {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                album_id: Uuid::from_slice(&album_id).unwrap_or_default().to_string(),
                provider: r.get("provider"),
                external_id: r.get("external_id"),
                external_url: r.get("external_url"),
                external_title: r.get("external_title"),
                cover_ref: r.get("cover_ref"),
            }
        })
        .collect())
}

/// Find a local album by a provider link's external_id.
pub(crate) async fn find_album_by_provider_link(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT album_id FROM album_provider_links WHERE provider = $1 AND external_id = $2",
    )
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("album_id");
        Uuid::from_slice(&id).unwrap_or_default().to_string()
    }))
}

#[allow(dead_code)]
pub(crate) async fn upsert_track_provider_link(
    pool: &SqlitePool,
    track_id: &str,
    provider: &str,
    external_id: &str,
) -> Result<(), sqlx::Error> {
    let link_uuid = new_uuid();
    let track_uuid = parse_uuid(track_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO track_provider_links (id, track_id, provider, external_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT(provider, external_id) DO UPDATE SET
           track_id = excluded.track_id",
    )
    .bind(link_uuid.as_bytes().as_slice())
    .bind(track_uuid.as_bytes().as_slice())
    .bind(provider)
    .bind(external_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ── Match suggestions ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct MatchSuggestion {
    pub(crate) id: String,
    pub(crate) scope_type: String,
    pub(crate) scope_id: String,
    pub(crate) left_provider: String,
    pub(crate) left_external_id: String,
    pub(crate) right_provider: String,
    pub(crate) right_external_id: String,
    pub(crate) match_kind: String,
    pub(crate) confidence: u8,
    pub(crate) explanation: Option<String>,
    pub(crate) external_name: Option<String>,
    pub(crate) external_url: Option<String>,
    pub(crate) image_ref: Option<String>,
    pub(crate) disambiguation: Option<String>,
    pub(crate) artist_type: Option<String>,
    pub(crate) country: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) popularity: Option<u8>,
    pub(crate) status: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

pub(crate) async fn upsert_match_suggestion(
    pool: &SqlitePool,
    suggestion: &MatchSuggestion,
) -> Result<(), sqlx::Error> {
    let id_uuid = parse_uuid(&suggestion.id).unwrap_or_else(|_| new_uuid());
    let scope_uuid = parse_uuid(&suggestion.scope_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO match_suggestions (
            id, scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country, tags_json, popularity,
            status, created_at, updated_at
         ) VALUES (
            $1, $2, $3,
            $4, $5,
            $6, $7,
            $8, $9, $10,
            $11, $12, $13,
            $14, $15, $16, $17, $18,
            $19, $20, $21
         )
         ON CONFLICT(
            scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind
         ) DO UPDATE SET
            confidence = excluded.confidence,
            explanation = excluded.explanation,
            external_name = excluded.external_name,
            external_url = excluded.external_url,
            image_ref = excluded.image_ref,
            disambiguation = excluded.disambiguation,
            artist_type = excluded.artist_type,
            country = excluded.country,
            tags_json = excluded.tags_json,
            popularity = excluded.popularity,
            status = excluded.status,
            updated_at = excluded.updated_at",
    )
    .bind(id_uuid.as_bytes().as_slice())
    .bind(&suggestion.scope_type)
    .bind(scope_uuid.as_bytes().as_slice())
    .bind(&suggestion.left_provider)
    .bind(&suggestion.left_external_id)
    .bind(&suggestion.right_provider)
    .bind(&suggestion.right_external_id)
    .bind(&suggestion.match_kind)
    .bind(i32::from(suggestion.confidence))
    .bind(&suggestion.explanation)
    .bind(&suggestion.external_name)
    .bind(&suggestion.external_url)
    .bind(&suggestion.image_ref)
    .bind(&suggestion.disambiguation)
    .bind(&suggestion.artist_type)
    .bind(&suggestion.country)
    .bind(json!(suggestion.tags).to_string())
    .bind(suggestion.popularity.map(i32::from))
    .bind(&suggestion.status)
    .bind(suggestion.created_at.to_rfc3339())
    .bind(suggestion.updated_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn clear_pending_match_suggestions(
    pool: &SqlitePool,
    scope_type: &str,
    scope_id: &str,
) -> Result<u64, sqlx::Error> {
    let scope_uuid = parse_uuid(scope_id).unwrap_or_default();
    let result = sqlx::query(
        "DELETE FROM match_suggestions
         WHERE scope_type = $1 AND scope_id = $2 AND status = 'pending'",
    )
    .bind(scope_type)
    .bind(scope_uuid.as_bytes().as_slice())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn load_match_suggestions_for_scope(
    pool: &SqlitePool,
    scope_type: &str,
    scope_id: &str,
) -> Result<Vec<MatchSuggestion>, sqlx::Error> {
    let scope_uuid = parse_uuid(scope_id).unwrap_or_default();
    let rows = sqlx::query(
        "SELECT
            id, scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country, tags_json, popularity,
            status,
            created_at, updated_at
         FROM match_suggestions
         WHERE scope_type = $1 AND scope_id = $2
         ORDER BY status ASC, confidence DESC, created_at DESC",
    )
    .bind(scope_type)
    .bind(scope_uuid.as_bytes().as_slice())
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let scope_id: Vec<u8> = r.get("scope_id");
            MatchSuggestion {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                scope_type: r.get("scope_type"),
                scope_id: Uuid::from_slice(&scope_id).unwrap_or_default().to_string(),
                left_provider: r.get("left_provider"),
                left_external_id: r.get("left_external_id"),
                right_provider: r.get("right_provider"),
                right_external_id: r.get("right_external_id"),
                match_kind: r.get("match_kind"),
                confidence: r.get::<i32, _>("confidence") as u8,
                explanation: r.get("explanation"),
                external_name: r.get("external_name"),
                external_url: r.get("external_url"),
                image_ref: r.get("image_ref"),
                disambiguation: r.get("disambiguation"),
                artist_type: r.get("artist_type"),
                country: r.get("country"),
                tags: r
                    .get::<Option<String>, _>("tags_json")
                    .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
                    .unwrap_or_default(),
                popularity: r.get::<Option<i32>, _>("popularity").map(|v| v as u8),
                status: r.get("status"),
                created_at: parse_dt(r.get::<String, _>("created_at")),
                updated_at: parse_dt(r.get::<String, _>("updated_at")),
            }
        })
        .collect())
}

pub(crate) async fn load_match_suggestion_by_id(
    pool: &SqlitePool,
    suggestion_id: &str,
) -> Result<Option<MatchSuggestion>, sqlx::Error> {
    let suggestion_uuid = parse_uuid(suggestion_id).unwrap_or_default();
    let row = sqlx::query(
        "SELECT
            id, scope_type, scope_id,
            left_provider, left_external_id,
            right_provider, right_external_id,
            match_kind, confidence, explanation,
            external_name, external_url, image_ref,
            disambiguation, artist_type, country, tags_json, popularity,
            status,
            created_at, updated_at
         FROM match_suggestions
         WHERE id = $1",
    )
    .bind(suggestion_uuid.as_bytes().as_slice())
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("id");
        let scope_id: Vec<u8> = r.get("scope_id");
        MatchSuggestion {
            id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
            scope_type: r.get("scope_type"),
            scope_id: Uuid::from_slice(&scope_id).unwrap_or_default().to_string(),
            left_provider: r.get("left_provider"),
            left_external_id: r.get("left_external_id"),
            right_provider: r.get("right_provider"),
            right_external_id: r.get("right_external_id"),
            match_kind: r.get("match_kind"),
            confidence: r.get::<i32, _>("confidence") as u8,
            explanation: r.get("explanation"),
            external_name: r.get("external_name"),
            external_url: r.get("external_url"),
            image_ref: r.get("image_ref"),
            disambiguation: r.get("disambiguation"),
            artist_type: r.get("artist_type"),
            country: r.get("country"),
            tags: r
                .get::<Option<String>, _>("tags_json")
                .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
                .unwrap_or_default(),
            popularity: r.get::<Option<i32>, _>("popularity").map(|v| v as u8),
            status: r.get("status"),
            created_at: parse_dt(r.get::<String, _>("created_at")),
            updated_at: parse_dt(r.get::<String, _>("updated_at")),
        }
    }))
}

pub(crate) async fn set_match_suggestion_status(
    pool: &SqlitePool,
    suggestion_id: &str,
    status: &str,
) -> Result<(), sqlx::Error> {
    let suggestion_uuid = parse_uuid(suggestion_id).unwrap_or_default();
    sqlx::query(
        "UPDATE match_suggestions
         SET status = $1, updated_at = $2
         WHERE id = $3",
    )
    .bind(status)
    .bind(Utc::now().to_rfc3339())
    .bind(suggestion_uuid.as_bytes().as_slice())
    .execute(pool)
    .await?;
    Ok(())
}

// ── Download jobs ───────────────────────────────────────────────────

pub(crate) async fn load_jobs(pool: &SqlitePool) -> Result<Vec<DownloadJob>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, album_id, source, album_title, artist_name, status, quality,
                total_tracks, completed_tracks, error, created_at, updated_at
         FROM download_jobs ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            let album_id: Vec<u8> = r.get("album_id");
            DownloadJob {
                id: Uuid::from_slice(&id).unwrap_or_default().to_string(),
                album_id: Uuid::from_slice(&album_id).unwrap_or_default().to_string(),
                source: r.get("source"),
                album_title: r.get("album_title"),
                artist_name: r.get("artist_name"),
                status: parse_status(&r.get::<String, _>("status")),
                quality: r.get("quality"),
                total_tracks: r.get::<i32, _>("total_tracks") as usize,
                completed_tracks: r.get::<i32, _>("completed_tracks") as usize,
                error: r.get("error"),
                created_at: parse_dt(r.get::<String, _>("created_at")),
                updated_at: parse_dt(r.get::<String, _>("updated_at")),
            }
        })
        .collect())
}

pub(crate) async fn insert_job(
    pool: &SqlitePool,
    job: &DownloadJob,
) -> Result<String, sqlx::Error> {
    let uuid = parse_uuid(&job.id).unwrap_or_else(|_| new_uuid());
    let album_uuid = parse_uuid(&job.album_id).unwrap_or_default();
    sqlx::query(
        "INSERT INTO download_jobs (id, album_id, source, album_title, artist_name, status, quality,
                                    total_tracks, completed_tracks, error, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(uuid.as_bytes().as_slice())
    .bind(album_uuid.as_bytes().as_slice())
    .bind(&job.source)
    .bind(&job.album_title)
    .bind(&job.artist_name)
    .bind(job.status.as_str())
    .bind(&job.quality)
    .bind(job.total_tracks as i32)
    .bind(job.completed_tracks as i32)
    .bind(&job.error)
    .bind(job.created_at.to_rfc3339())
    .bind(job.updated_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(uuid.to_string())
}

pub(crate) async fn update_job(pool: &SqlitePool, job: &DownloadJob) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(&job.id).unwrap_or_default();
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
    .bind(uuid.as_bytes().as_slice())
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn delete_job(pool: &SqlitePool, job_id: &str) -> Result<(), sqlx::Error> {
    let uuid = parse_uuid(job_id).unwrap_or_default();
    sqlx::query("DELETE FROM download_jobs WHERE id = $1")
        .bind(uuid.as_bytes().as_slice())
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
