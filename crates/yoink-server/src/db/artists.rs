use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::MonitoredArtist;

use super::parse_dt;

pub(crate) async fn load_artists(pool: &SqlitePool) -> Result<Vec<MonitoredArtist>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, image_url, bio, added_at FROM artists ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let id: Vec<u8> = r.get("id");
            MonitoredArtist {
                id: Uuid::from_slice(&id).unwrap_or_default(),
                name: r.get("name"),
                image_url: r.get("image_url"),
                bio: r.get("bio"),
                added_at: parse_dt(r.get::<String, _>("added_at")),
            }
        })
        .collect())
}

pub(crate) async fn upsert_artist(
    pool: &SqlitePool,
    artist: &MonitoredArtist,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO artists (id, name, image_url, bio, added_at)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           image_url = excluded.image_url,
           bio = COALESCE(excluded.bio, artists.bio)",
    )
    .bind(artist.id.as_bytes().as_slice())
    .bind(&artist.name)
    .bind(&artist.image_url)
    .bind(&artist.bio)
    .bind(artist.added_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Update only the bio field for an artist.
pub(crate) async fn update_artist_bio(
    pool: &SqlitePool,
    artist_id: Uuid,
    bio: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE artists SET bio = $1 WHERE id = $2")
        .bind(bio)
        .bind(artist_id.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_artist(pool: &SqlitePool, artist_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM artists WHERE id = $1")
        .bind(artist_id.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}
