use std::collections::HashMap;

use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::MonitoredAlbum;

pub(crate) async fn load_albums(pool: &SqlitePool) -> Result<Vec<MonitoredAlbum>, sqlx::Error> {
    // 1. Load the raw album rows (artist_id = legacy primary artist).
    //    partially_wanted is derived: album is NOT fully monitored but has
    //    individually monitored tracks that are not yet acquired.
    struct AlbumRow {
        id: Uuid,
        artist_id: Uuid,
        title: String,
        album_type: Option<String>,
        release_date: Option<String>,
        cover_url: Option<String>,
        explicit: bool,
        monitored: bool,
        acquired: bool,
        wanted: bool,
        partially_wanted: bool,
        added_at: chrono::DateTime<chrono::Utc>,
        artist_credits: Option<String>,
    }

    let rows = sqlx::query_as!(
        AlbumRow,
        r#"SELECT
            a.id as "id!: Uuid",
            a.artist_id as "artist_id!: Uuid",
            a.title, a.album_type, a.release_date, a.cover_url,
            a.explicit as "explicit!: bool",
            a.monitored as "monitored!: bool",
            a.acquired as "acquired!: bool",
            a.wanted as "wanted!: bool",
            CASE WHEN a.monitored = 0 AND EXISTS (
                SELECT 1 FROM tracks t WHERE t.album_id = a.id AND t.monitored = 1 AND t.acquired = 0
            ) THEN 1 ELSE 0 END as "partially_wanted!: bool",
            a.added_at as "added_at!: chrono::DateTime<chrono::Utc>",
            a.artist_credits
         FROM albums a ORDER BY a.release_date DESC"#
    )
    .fetch_all(pool)
    .await?;

    // 2. Load artist associations from the junction table in one query.
    let all_pairs = super::album_artists::load_all_album_artist_ids(pool).await?;
    let mut artist_map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (album_id, artist_id) in all_pairs {
        artist_map.entry(album_id).or_default().push(artist_id);
    }

    // 3. Build MonitoredAlbum values, filling in artist_ids.
    let albums = rows
        .into_iter()
        .map(|r| {
            let artist_ids = artist_map
                .remove(&r.id)
                .unwrap_or_else(|| vec![r.artist_id]);
            let artist_credits: Vec<yoink_shared::ArtistCredit> = r
                .artist_credits
                .as_deref()
                .and_then(|json| serde_json::from_str(json).ok())
                .unwrap_or_default();
            MonitoredAlbum {
                id: r.id,
                artist_id: r.artist_id,
                artist_ids,
                artist_credits,
                title: r.title,
                album_type: r.album_type,
                release_date: r.release_date,
                cover_url: r.cover_url,
                explicit: r.explicit,
                monitored: r.monitored,
                acquired: r.acquired,
                wanted: r.wanted,
                partially_wanted: r.partially_wanted,
                added_at: r.added_at,
            }
        })
        .collect();

    Ok(albums)
}

pub(crate) async fn upsert_album(
    pool: &SqlitePool,
    album: &MonitoredAlbum,
) -> Result<(), sqlx::Error> {
    let artist_credits_json: Option<String> = if album.artist_credits.is_empty() {
        None
    } else {
        serde_json::to_string(&album.artist_credits).ok()
    };

    sqlx::query!(
        "INSERT INTO albums (id, artist_id, title, album_type, release_date, cover_url,
                             explicit, monitored, acquired, wanted, added_at, artist_credits)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         ON CONFLICT(id) DO UPDATE SET
           artist_id = excluded.artist_id,
           title = excluded.title,
           album_type = excluded.album_type,
           release_date = excluded.release_date,
           cover_url = excluded.cover_url,
           explicit = excluded.explicit,
           monitored = excluded.monitored,
           acquired = excluded.acquired,
           wanted = excluded.wanted,
           artist_credits = excluded.artist_credits",
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
        artist_credits_json,
    )
    .execute(pool)
    .await?;

    // Sync the junction table so it stays consistent with artist_ids.
    if !album.artist_ids.is_empty() {
        super::album_artists::set_album_artists(pool, album.id, &album.artist_ids).await?;
    } else {
        // Fallback: at minimum the primary artist must be in the junction table.
        super::album_artists::set_album_artists(pool, album.id, &[album.artist_id]).await?;
    }

    Ok(())
}

pub(crate) async fn delete_albums_by_artist(
    pool: &SqlitePool,
    artist_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM download_jobs
         WHERE album_id IN (
             SELECT id FROM albums WHERE artist_id = ?1
         )",
    )
    .bind(artist_id)
    .execute(pool)
    .await?;

    sqlx::query!("DELETE FROM albums WHERE artist_id = $1", artist_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_album(pool: &SqlitePool, album_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM download_jobs WHERE album_id = ?1")
        .bind(album_id)
        .execute(pool)
        .await?;

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

#[cfg(test)]
mod tests {
    use crate::test_helpers::*;

    #[tokio::test]
    async fn upsert_and_load_album() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let album = seed_album(&pool, artist.id, "Album One").await;

        let albums = super::load_albums(&pool).await.unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, album.id);
        assert_eq!(albums[0].title, "Album One");
        assert_eq!(albums[0].artist_id, artist.id);
        assert!(albums[0].monitored);
        assert!(albums[0].wanted);
        assert!(!albums[0].acquired);
    }

    #[tokio::test]
    async fn load_album_derives_partially_wanted() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let album = seed_album(&pool, artist.id, "Album").await;

        // Album is monitored by default. Set it to not-monitored so
        // partially_wanted can trigger.
        super::update_album_flags(&pool, album.id, false, false, false)
            .await
            .unwrap();

        // Add a track that is individually monitored but not acquired
        let tracks = seed_tracks(&pool, album.id, 2).await;
        crate::db::update_track_flags(&pool, tracks[0].id, true, false)
            .await
            .unwrap();

        let albums = super::load_albums(&pool).await.unwrap();
        assert_eq!(albums.len(), 1);
        assert!(albums[0].partially_wanted, "should be partially_wanted");
    }

    #[tokio::test]
    async fn partially_wanted_false_when_album_monitored() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let album = seed_album(&pool, artist.id, "Album").await;

        // Album is monitored=true (default from seed). Even with monitored
        // tracks, partially_wanted should be false because the album itself is
        // fully monitored.
        let tracks = seed_tracks(&pool, album.id, 2).await;
        crate::db::update_track_flags(&pool, tracks[0].id, true, false)
            .await
            .unwrap();

        let albums = super::load_albums(&pool).await.unwrap();
        assert!(!albums[0].partially_wanted);
    }

    #[tokio::test]
    async fn upsert_album_updates_on_conflict() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let mut album = seed_album(&pool, artist.id, "Original Title").await;

        album.title = "Updated Title".to_string();
        album.explicit = true;
        super::upsert_album(&pool, &album).await.unwrap();

        let albums = super::load_albums(&pool).await.unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Updated Title");
        assert!(albums[0].explicit);
    }

    #[tokio::test]
    async fn update_album_flags() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let album = seed_album(&pool, artist.id, "Album").await;

        super::update_album_flags(&pool, album.id, false, true, false)
            .await
            .unwrap();

        let albums = super::load_albums(&pool).await.unwrap();
        assert!(!albums[0].monitored);
        assert!(albums[0].acquired);
        assert!(!albums[0].wanted);
    }

    #[tokio::test]
    async fn delete_album() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        seed_album(&pool, artist.id, "Album").await;
        assert_eq!(super::load_albums(&pool).await.unwrap().len(), 1);

        let albums = super::load_albums(&pool).await.unwrap();
        super::delete_album(&pool, albums[0].id).await.unwrap();
        assert!(super::load_albums(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_album_removes_related_jobs() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let album = seed_album(&pool, artist.id, "Album").await;
        seed_job(&pool, album.id, crate::models::DownloadStatus::Queued).await;

        super::delete_album(&pool, album.id).await.unwrap();

        let jobs = crate::db::load_jobs(&pool).await.unwrap();
        assert!(jobs.is_empty());
    }

    #[tokio::test]
    async fn delete_albums_by_artist() {
        let pool = test_db().await;
        let artist1 = seed_artist(&pool, "Artist A").await;
        let artist2 = seed_artist(&pool, "Artist B").await;
        seed_album(&pool, artist1.id, "Album 1").await;
        seed_album(&pool, artist1.id, "Album 2").await;
        seed_album(&pool, artist2.id, "Album 3").await;

        super::delete_albums_by_artist(&pool, artist1.id)
            .await
            .unwrap();

        let albums = super::load_albums(&pool).await.unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Album 3");
    }

    #[tokio::test]
    async fn delete_albums_by_artist_removes_related_jobs() {
        let pool = test_db().await;
        let artist1 = seed_artist(&pool, "Artist A").await;
        let artist2 = seed_artist(&pool, "Artist B").await;
        let album1 = seed_album(&pool, artist1.id, "Album 1").await;
        let album2 = seed_album(&pool, artist2.id, "Album 2").await;
        seed_job(&pool, album1.id, crate::models::DownloadStatus::Queued).await;
        seed_job(&pool, album2.id, crate::models::DownloadStatus::Queued).await;

        super::delete_albums_by_artist(&pool, artist1.id)
            .await
            .unwrap();

        let jobs = crate::db::load_jobs(&pool).await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].album_id, album2.id);
    }

    #[tokio::test]
    async fn artist_credits_json_roundtrip() {
        use yoink_shared::ArtistCredit;

        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let mut album = seed_album(&pool, artist.id, "Collab Album").await;

        album.artist_credits = vec![
            ArtistCredit {
                name: "Artist A".to_string(),
                provider: Some("tidal".to_string()),
                external_id: Some("111".to_string()),
            },
            ArtistCredit {
                name: "Artist B".to_string(),
                provider: None,
                external_id: None,
            },
        ];
        super::upsert_album(&pool, &album).await.unwrap();

        let albums = super::load_albums(&pool).await.unwrap();
        assert_eq!(albums[0].artist_credits.len(), 2);
        assert_eq!(albums[0].artist_credits[0].name, "Artist A");
        assert_eq!(
            albums[0].artist_credits[0].provider.as_deref(),
            Some("tidal")
        );
        assert_eq!(albums[0].artist_credits[1].name, "Artist B");
        assert!(albums[0].artist_credits[1].provider.is_none());
    }

    #[tokio::test]
    async fn load_albums_sorted_by_release_date_desc() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;

        let mut a1 = seed_album(&pool, artist.id, "Older").await;
        a1.release_date = Some("2020-01-01".to_string());
        super::upsert_album(&pool, &a1).await.unwrap();

        let mut a2 = seed_album(&pool, artist.id, "Newer").await;
        a2.release_date = Some("2024-06-15".to_string());
        super::upsert_album(&pool, &a2).await.unwrap();

        let albums = super::load_albums(&pool).await.unwrap();
        assert_eq!(albums[0].title, "Newer");
        assert_eq!(albums[1].title, "Older");
    }

    #[tokio::test]
    async fn reassign_tracks_to_album() {
        let pool = test_db().await;
        let artist = seed_artist(&pool, "Artist").await;
        let album_a = seed_album(&pool, artist.id, "Source Album").await;
        let album_b = seed_album(&pool, artist.id, "Target Album").await;
        seed_tracks(&pool, album_a.id, 3).await;

        let moved = super::reassign_tracks_to_album(&pool, album_a.id, album_b.id)
            .await
            .unwrap();
        assert_eq!(moved, 3);

        let tracks_a = crate::db::load_tracks_for_album(&pool, album_a.id)
            .await
            .unwrap();
        let tracks_b = crate::db::load_tracks_for_album(&pool, album_b.id)
            .await
            .unwrap();
        assert!(tracks_a.is_empty());
        assert_eq!(tracks_b.len(), 3);
    }
}
