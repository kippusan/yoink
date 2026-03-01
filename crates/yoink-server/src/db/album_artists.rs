use sqlx::SqlitePool;
use uuid::Uuid;

/// Load all artist IDs for an album, ordered by `ordering`.
#[allow(dead_code)]
pub(crate) async fn load_album_artist_ids(
    pool: &SqlitePool,
    album_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query_scalar!(
        r#"SELECT artist_id as "artist_id!: Uuid"
           FROM album_artists
           WHERE album_id = $1
           ORDER BY ordering"#,
        album_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Load the artist IDs for every album in one query, returned as
/// `(album_id, artist_id)` pairs sorted by `(album_id, ordering)`.
pub(crate) async fn load_all_album_artist_ids(
    pool: &SqlitePool,
) -> Result<Vec<(Uuid, Uuid)>, sqlx::Error> {
    struct Row {
        album_id: Uuid,
        artist_id: Uuid,
    }
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT album_id as "album_id!: Uuid", artist_id as "artist_id!: Uuid"
           FROM album_artists
           ORDER BY album_id, ordering"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.album_id, r.artist_id)).collect())
}

/// Replace the full set of artists for an album.
/// `artist_ids` order determines display ordering (index = ordering value).
pub(crate) async fn set_album_artists(
    pool: &SqlitePool,
    album_id: Uuid,
    artist_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    // Delete existing entries
    sqlx::query!("DELETE FROM album_artists WHERE album_id = $1", album_id)
        .execute(pool)
        .await?;

    // Insert new entries
    for (idx, artist_id) in artist_ids.iter().enumerate() {
        let ordering = idx as i32;
        sqlx::query!(
            "INSERT INTO album_artists (album_id, artist_id, ordering) VALUES ($1, $2, $3)",
            album_id,
            artist_id,
            ordering,
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Add an artist to an album (appended at the end).
pub(crate) async fn add_album_artist(
    pool: &SqlitePool,
    album_id: Uuid,
    artist_id: Uuid,
) -> Result<(), sqlx::Error> {
    let max_ordering: Option<i32> = sqlx::query_scalar!(
        r#"SELECT MAX(ordering) as "max_ordering: i32" FROM album_artists WHERE album_id = $1"#,
        album_id,
    )
    .fetch_one(pool)
    .await?;

    let next = max_ordering.unwrap_or(-1) + 1;
    sqlx::query!(
        "INSERT OR IGNORE INTO album_artists (album_id, artist_id, ordering) VALUES ($1, $2, $3)",
        album_id,
        artist_id,
        next,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove an artist from an album.
pub(crate) async fn remove_album_artist(
    pool: &SqlitePool,
    album_id: Uuid,
    artist_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM album_artists WHERE album_id = $1 AND artist_id = $2",
        album_id,
        artist_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete all album_artists entries for albums belonging to a given artist.
/// (Used when removing an artist — cascade handles this via FK, but explicit
/// cleanup may be needed for the in-memory state.)
pub(crate) async fn delete_album_artists_by_artist(
    pool: &SqlitePool,
    artist_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM album_artists WHERE artist_id = $1",
        artist_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}
