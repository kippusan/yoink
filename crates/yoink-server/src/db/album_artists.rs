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
    Ok(rows
        .into_iter()
        .map(|r| (r.album_id, r.artist_id))
        .collect())
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
    sqlx::query!("DELETE FROM album_artists WHERE artist_id = $1", artist_id,)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test_helpers::*;

    #[tokio::test]
    async fn set_album_artists_replaces_all() {
        let pool = test_db().await;
        let a1 = seed_artist(&pool, "Artist 1").await;
        let a2 = seed_artist(&pool, "Artist 2").await;
        let a3 = seed_artist(&pool, "Artist 3").await;
        let album = seed_album(&pool, a1.id, "Collab").await;

        // seed_album already sets [a1] via upsert_album.
        // Now replace with [a2, a3, a1]
        super::set_album_artists(&pool, album.id, &[a2.id, a3.id, a1.id])
            .await
            .unwrap();

        let ids = super::load_album_artist_ids(&pool, album.id).await.unwrap();
        assert_eq!(ids, vec![a2.id, a3.id, a1.id]);
    }

    #[tokio::test]
    async fn add_album_artist_appends() {
        let pool = test_db().await;
        let a1 = seed_artist(&pool, "Artist 1").await;
        let a2 = seed_artist(&pool, "Artist 2").await;
        let album = seed_album(&pool, a1.id, "Album").await;

        super::add_album_artist(&pool, album.id, a2.id)
            .await
            .unwrap();

        let ids = super::load_album_artist_ids(&pool, album.id).await.unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], a1.id);
        assert_eq!(ids[1], a2.id);
    }

    #[tokio::test]
    async fn add_album_artist_ignores_duplicate() {
        let pool = test_db().await;
        let a1 = seed_artist(&pool, "Artist 1").await;
        let album = seed_album(&pool, a1.id, "Album").await;

        // a1 is already in the junction table from seed_album
        super::add_album_artist(&pool, album.id, a1.id)
            .await
            .unwrap();

        let ids = super::load_album_artist_ids(&pool, album.id).await.unwrap();
        assert_eq!(ids.len(), 1); // Not duplicated
    }

    #[tokio::test]
    async fn remove_album_artist() {
        let pool = test_db().await;
        let a1 = seed_artist(&pool, "Artist 1").await;
        let a2 = seed_artist(&pool, "Artist 2").await;
        let album = seed_album(&pool, a1.id, "Album").await;
        super::add_album_artist(&pool, album.id, a2.id)
            .await
            .unwrap();

        super::remove_album_artist(&pool, album.id, a1.id)
            .await
            .unwrap();

        let ids = super::load_album_artist_ids(&pool, album.id).await.unwrap();
        assert_eq!(ids, vec![a2.id]);
    }

    #[tokio::test]
    async fn load_all_album_artist_ids() {
        let pool = test_db().await;
        let a1 = seed_artist(&pool, "Artist 1").await;
        let a2 = seed_artist(&pool, "Artist 2").await;
        let album1 = seed_album(&pool, a1.id, "Album 1").await;
        let album2 = seed_album(&pool, a2.id, "Album 2").await;
        super::add_album_artist(&pool, album1.id, a2.id)
            .await
            .unwrap();

        let all = super::load_all_album_artist_ids(&pool).await.unwrap();
        // album1 has [a1, a2], album2 has [a2]
        let album1_artists: Vec<_> = all
            .iter()
            .filter(|(alb, _)| *alb == album1.id)
            .map(|(_, art)| *art)
            .collect();
        assert_eq!(album1_artists, vec![a1.id, a2.id]);

        let album2_artists: Vec<_> = all
            .iter()
            .filter(|(alb, _)| *alb == album2.id)
            .map(|(_, art)| *art)
            .collect();
        assert_eq!(album2_artists, vec![a2.id]);
    }

    #[tokio::test]
    async fn delete_album_artists_by_artist() {
        let pool = test_db().await;
        let a1 = seed_artist(&pool, "Artist 1").await;
        let a2 = seed_artist(&pool, "Artist 2").await;
        let album = seed_album(&pool, a1.id, "Album").await;
        super::add_album_artist(&pool, album.id, a2.id)
            .await
            .unwrap();

        super::delete_album_artists_by_artist(&pool, a1.id)
            .await
            .unwrap();

        let ids = super::load_album_artist_ids(&pool, album.id).await.unwrap();
        assert_eq!(ids, vec![a2.id]); // Only a2 remains
    }
}
