use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::MonitoredAlbum;

use super::parse_dt;

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
                id: Uuid::from_slice(&id).unwrap_or_default(),
                artist_id: Uuid::from_slice(&artist_id).unwrap_or_default(),
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
    .bind(album.id.as_bytes().as_slice())
    .bind(album.artist_id.as_bytes().as_slice())
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
    artist_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM albums WHERE artist_id = $1")
        .bind(artist_id.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_album(pool: &SqlitePool, album_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM albums WHERE id = $1")
        .bind(album_id.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn update_album_flags(
    pool: &SqlitePool,
    album_id: Uuid,
    monitored: bool,
    acquired: bool,
    wanted: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE albums SET monitored = $1, acquired = $2, wanted = $3 WHERE id = $4")
        .bind(monitored as i32)
        .bind(acquired as i32)
        .bind(wanted as i32)
        .bind(album_id.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn reassign_tracks_to_album(
    pool: &SqlitePool,
    from_album_id: Uuid,
    to_album_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE tracks SET album_id = $1 WHERE album_id = $2")
        .bind(to_album_id.as_bytes().as_slice())
        .bind(from_album_id.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn reassign_jobs_to_album(
    pool: &SqlitePool,
    from_album_id: Uuid,
    to_album_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("UPDATE download_jobs SET album_id = $1 WHERE album_id = $2")
        .bind(to_album_id.as_bytes().as_slice())
        .bind(from_album_id.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
