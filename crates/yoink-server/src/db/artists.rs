use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::MonitoredArtist;

pub(crate) async fn load_artists(pool: &SqlitePool) -> Result<Vec<MonitoredArtist>, sqlx::Error> {
    sqlx::query_as!(
        MonitoredArtist,
        r#"SELECT id as "id!: Uuid", name, image_url, bio, added_at as "added_at!: chrono::DateTime<chrono::Utc>"
           FROM artists ORDER BY name COLLATE NOCASE"#
    )
    .fetch_all(pool)
    .await
}

pub(crate) async fn upsert_artist(
    pool: &SqlitePool,
    artist: &MonitoredArtist,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO artists (id, name, image_url, bio, added_at)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           image_url = excluded.image_url,
           bio = COALESCE(excluded.bio, artists.bio)",
        artist.id,
        artist.name,
        artist.image_url,
        artist.bio,
        artist.added_at,
    )
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
    sqlx::query!("UPDATE artists SET bio = $1 WHERE id = $2", bio, artist_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update the artist name and/or image_url. Only provided (Some) fields are updated.
pub(crate) async fn update_artist_details(
    pool: &SqlitePool,
    artist_id: Uuid,
    name: Option<&str>,
    image_url: Option<Option<&str>>,
) -> Result<(), sqlx::Error> {
    if let Some(new_name) = name {
        sqlx::query!(
            "UPDATE artists SET name = $1 WHERE id = $2",
            new_name,
            artist_id
        )
        .execute(pool)
        .await?;
    }
    if let Some(new_image) = image_url {
        sqlx::query!(
            "UPDATE artists SET image_url = $1 WHERE id = $2",
            new_image,
            artist_id
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn delete_artist(pool: &SqlitePool, artist_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM artists WHERE id = $1", artist_id)
        .execute(pool)
        .await?;
    Ok(())
}
