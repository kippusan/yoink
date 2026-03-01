use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::TrackInfo;

pub(crate) async fn load_tracks_for_album(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<Vec<TrackInfo>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, title, version, disc_number, track_number, duration_secs, explicit, isrc
         FROM tracks WHERE album_id = $1
         ORDER BY disc_number, track_number",
    )
    .bind(album_id.as_bytes().as_slice())
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
                id: Uuid::from_slice(&id).unwrap_or_default(),
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
    album_id: Uuid,
) -> Result<(), sqlx::Error> {
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
    .bind(track.id.as_bytes().as_slice())
    .bind(album_id.as_bytes().as_slice())
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
) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT track_id FROM track_provider_links WHERE provider = $1 AND external_id = $2",
    )
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("track_id");
        Uuid::from_slice(&id).unwrap_or_default()
    }))
}

pub(crate) async fn find_track_by_album_isrc(
    pool: &SqlitePool,
    album_id: Uuid,
    isrc: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id FROM tracks WHERE album_id = $1 AND UPPER(isrc) = UPPER($2) LIMIT 1",
    )
    .bind(album_id.as_bytes().as_slice())
    .bind(isrc)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("id");
        Uuid::from_slice(&id).unwrap_or_default()
    }))
}

pub(crate) async fn find_track_by_album_position(
    pool: &SqlitePool,
    album_id: Uuid,
    disc_number: u32,
    track_number: u32,
) -> Result<Option<Uuid>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id
         FROM tracks
         WHERE album_id = $1 AND disc_number = $2 AND track_number = $3
         LIMIT 1",
    )
    .bind(album_id.as_bytes().as_slice())
    .bind(disc_number as i32)
    .bind(track_number as i32)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let id: Vec<u8> = r.get("id");
        Uuid::from_slice(&id).unwrap_or_default()
    }))
}
