use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::MonitoredAlbum;

pub(crate) async fn load_albums(pool: &SqlitePool) -> Result<Vec<MonitoredAlbum>, sqlx::Error> {
    sqlx::query_as!(
        MonitoredAlbum,
        r#"SELECT
            id as "id!: Uuid",
            artist_id as "artist_id!: Uuid",
            title, album_type, release_date, cover_url,
            explicit as "explicit!: bool",
            monitored as "monitored!: bool",
            acquired as "acquired!: bool",
            wanted as "wanted!: bool",
            added_at as "added_at!: chrono::DateTime<chrono::Utc>"
         FROM albums ORDER BY release_date DESC"#
    )
    .fetch_all(pool)
    .await
}

pub(crate) async fn upsert_album(
    pool: &SqlitePool,
    album: &MonitoredAlbum,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
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
        album.id,
        album.artist_id,
        album.title,
        album.album_type,
        album.release_date,
        album.cover_url,
        album.explicit,
        album.monitored,
        album.acquired,
        album.wanted,
        album.added_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_albums_by_artist(
    pool: &SqlitePool,
    artist_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM albums WHERE artist_id = $1", artist_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_album(pool: &SqlitePool, album_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM albums WHERE id = $1", album_id)
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
    sqlx::query!(
        "UPDATE albums SET monitored = $1, acquired = $2, wanted = $3 WHERE id = $4",
        monitored,
        acquired,
        wanted,
        album_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn reassign_tracks_to_album(
    pool: &SqlitePool,
    from_album_id: Uuid,
    to_album_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE tracks SET album_id = $1 WHERE album_id = $2",
        to_album_id,
        from_album_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn reassign_jobs_to_album(
    pool: &SqlitePool,
    from_album_id: Uuid,
    to_album_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE download_jobs SET album_id = $1 WHERE album_id = $2",
        to_album_id,
        from_album_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
