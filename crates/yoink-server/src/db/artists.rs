use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::MonitoredArtist;

pub(crate) async fn load_artists(pool: &SqlitePool) -> Result<Vec<MonitoredArtist>, sqlx::Error> {
    sqlx::query_as!(
        MonitoredArtist,
        r#"SELECT id as "id!: Uuid", name, image_url, bio,
                  monitored as "monitored!: bool",
                  added_at as "added_at!: chrono::DateTime<chrono::Utc>"
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
        "INSERT INTO artists (id, name, image_url, bio, monitored, added_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           image_url = excluded.image_url,
           bio = COALESCE(excluded.bio, artists.bio),
           monitored = excluded.monitored",
        artist.id,
        artist.name,
        artist.image_url,
        artist.bio,
        artist.monitored,
        artist.added_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Update only the monitored flag for an artist.
pub(crate) async fn update_artist_monitored(
    pool: &SqlitePool,
    artist_id: Uuid,
    monitored: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE artists SET monitored = $1 WHERE id = $2",
        monitored,
        artist_id
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

#[cfg(test)]
mod tests {
    use crate::test_helpers::*;

    #[tokio::test]
    async fn insert_and_load_artist() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Radiohead").await;

        let artists = super::load_artists(&pool).await.unwrap();
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].id, artist.id);
        assert_eq!(artists[0].name, "Radiohead");
        assert!(artists[0].monitored);
    }

    #[tokio::test]
    async fn upsert_artist_updates_on_conflict() {
        let pool = test_db().await;
        let mut artist = seed_artist(&pool, "Radiohead").await;

        // Update name via upsert
        artist.name = "Radiohead (Updated)".to_string();
        super::upsert_artist(&pool, &artist).await.unwrap();

        let artists = super::load_artists(&pool).await.unwrap();
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Radiohead (Updated)");
    }

    #[tokio::test]
    async fn upsert_artist_preserves_existing_bio_when_new_is_none() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;

        // Set bio directly
        super::update_artist_bio(&pool, artist.id, Some("Great band"))
            .await
            .unwrap();

        // Upsert with bio = None should keep the existing bio (COALESCE behavior)
        let mut updated = artist.clone();
        updated.bio = None;
        updated.name = "Artist Renamed".to_string();
        super::upsert_artist(&pool, &updated).await.unwrap();

        let loaded = super::load_artists(&pool).await.unwrap();
        assert_eq!(loaded[0].name, "Artist Renamed");
        assert_eq!(loaded[0].bio.as_deref(), Some("Great band"));
    }

    #[tokio::test]
    async fn update_artist_bio() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;

        super::update_artist_bio(&pool, artist.id, Some("A biography"))
            .await
            .unwrap();
        let loaded = super::load_artists(&pool).await.unwrap();
        assert_eq!(loaded[0].bio.as_deref(), Some("A biography"));

        // Clear bio
        super::update_artist_bio(&pool, artist.id, None)
            .await
            .unwrap();
        let loaded = super::load_artists(&pool).await.unwrap();
        assert!(loaded[0].bio.is_none());
    }

    #[tokio::test]
    async fn update_artist_details_name_only() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Old Name").await;

        super::update_artist_details(&pool, artist.id, Some("New Name"), None)
            .await
            .unwrap();

        let loaded = super::load_artists(&pool).await.unwrap();
        assert_eq!(loaded[0].name, "New Name");
        assert!(loaded[0].image_url.is_none());
    }

    #[tokio::test]
    async fn update_artist_details_image_only() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;

        super::update_artist_details(
            &pool,
            artist.id,
            None,
            Some(Some("https://example.com/img.jpg")),
        )
        .await
        .unwrap();

        let loaded = super::load_artists(&pool).await.unwrap();
        assert_eq!(loaded[0].name, "Artist");
        assert_eq!(
            loaded[0].image_url.as_deref(),
            Some("https://example.com/img.jpg")
        );
    }

    #[tokio::test]
    async fn update_artist_monitored_flag() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        assert!(artist.monitored);

        super::update_artist_monitored(&pool, artist.id, false)
            .await
            .unwrap();
        let loaded = super::load_artists(&pool).await.unwrap();
        assert!(!loaded[0].monitored);
    }

    #[tokio::test]
    async fn delete_artist_removes_row() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Deletable").await;
        assert_eq!(super::load_artists(&pool).await.unwrap().len(), 1);

        super::delete_artist(&pool, artist.id).await.unwrap();
        assert!(super::load_artists(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn load_artists_sorted_case_insensitive() {
        let pool = test_db().await;
        seed_artist(&pool, "Zedd").await;
        seed_artist(&pool, "avicii").await;
        seed_artist(&pool, "Aphex Twin").await;

        let artists = super::load_artists(&pool).await.unwrap();
        let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Aphex Twin", "avicii", "Zedd"]);
    }
}
