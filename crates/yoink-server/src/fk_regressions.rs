use std::sync::Arc;

use crate::actions::dispatch_action_impl;
use crate::db;
use crate::providers::registry::ProviderRegistry;
use crate::providers::{MetadataProvider, ProviderAlbum, ProviderAlbumArtist};
use crate::services;
use crate::test_helpers::{
    MockMetadataProvider, seed_album, seed_artist, seed_artist_provider_link, seed_job,
    test_app_state, test_app_state_with_registry,
};
use yoink_shared::ServerAction;

async fn assert_no_foreign_key_violations(pool: &sqlx::SqlitePool) {
    let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
        .fetch_one(pool)
        .await
        .expect("pragma_foreign_key_check query should succeed");
    assert_eq!(
        violations, 0,
        "database has foreign-key violations after operation"
    );
}

#[tokio::test]
async fn checklist_remove_artist_with_jobs_no_fk_error_and_unrelated_data_survives() {
    let (state, _tmp) = test_app_state().await;
    let a1 = seed_artist(&state.db, "Artist 1").await;
    let a2 = seed_artist(&state.db, "Artist 2").await;
    let album1 = seed_album(&state.db, a1.id, "Album 1").await;
    let album2 = seed_album(&state.db, a2.id, "Album 2").await;
    let job1 = seed_job(&state.db, album1.id, crate::models::DownloadStatus::Queued).await;
    let job2 = seed_job(&state.db, album2.id, crate::models::DownloadStatus::Queued).await;

    {
        let mut artists = state.monitored_artists.write().await;
        artists.push(a1.clone());
        artists.push(a2.clone());
    }
    {
        let mut albums = state.monitored_albums.write().await;
        albums.push(album1.clone());
        albums.push(album2.clone());
    }
    {
        let mut jobs = state.download_jobs.write().await;
        jobs.push(job1);
        jobs.push(job2);
    }

    dispatch_action_impl(
        state.clone(),
        ServerAction::RemoveArtist {
            artist_id: a1.id,
            remove_files: false,
        },
    )
    .await
    .expect("remove artist should not fail due to FK constraints");

    assert_no_foreign_key_violations(&state.db).await;

    let artists = db::load_artists(&state.db).await.unwrap();
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].id, a2.id);

    let albums = db::load_albums(&state.db).await.unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].id, album2.id);

    let jobs = db::load_jobs(&state.db).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].album_id, album2.id);
}

#[tokio::test]
async fn checklist_delete_album_db_helper_no_fk_error_and_unrelated_jobs_survive() {
    let (state, _tmp) = test_app_state().await;
    let a1 = seed_artist(&state.db, "Artist 1").await;
    let a2 = seed_artist(&state.db, "Artist 2").await;
    let album1 = seed_album(&state.db, a1.id, "Album 1").await;
    let album2 = seed_album(&state.db, a2.id, "Album 2").await;
    seed_job(&state.db, album1.id, crate::models::DownloadStatus::Queued).await;
    seed_job(&state.db, album2.id, crate::models::DownloadStatus::Queued).await;

    db::delete_album(&state.db, album1.id)
        .await
        .expect("delete album should clean dependent rows first");

    assert_no_foreign_key_violations(&state.db).await;

    let albums = db::load_albums(&state.db).await.unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].id, album2.id);

    let jobs = db::load_jobs(&state.db).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].album_id, album2.id);
}

#[tokio::test]
async fn checklist_sync_cleanup_no_fk_error_and_unrelated_jobs_survive() {
    let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
    *mock.fetch_albums_result.lock().await = Ok(vec![ProviderAlbum {
        external_id: "incoming_1".to_string(),
        title: "Fresh Album".to_string(),
        album_type: Some("album".to_string()),
        release_date: Some("2024-04-20".to_string()),
        cover_ref: None,
        url: None,
        explicit: false,
        artists: vec![ProviderAlbumArtist {
            external_id: "artist_ext_1".to_string(),
            name: "Artist 1".to_string(),
        }],
    }]);

    let mut registry = ProviderRegistry::new();
    registry.register_metadata(mock as Arc<dyn MetadataProvider>);
    let (state, _tmp) = test_app_state_with_registry(registry).await;

    let a1 = seed_artist(&state.db, "Artist 1").await;
    let a2 = seed_artist(&state.db, "Artist 2").await;
    seed_artist_provider_link(&state.db, a1.id, "mock_prov", "artist_ext_1").await;

    let stale = seed_album(&state.db, a1.id, "Stale Album").await;
    let keep = seed_album(&state.db, a2.id, "Keep Album").await;
    let stale_job = seed_job(&state.db, stale.id, crate::models::DownloadStatus::Queued).await;
    let keep_job = seed_job(&state.db, keep.id, crate::models::DownloadStatus::Queued).await;

    {
        let mut artists = state.monitored_artists.write().await;
        artists.push(a1.clone());
        artists.push(a2.clone());
    }
    {
        let mut albums = state.monitored_albums.write().await;
        albums.push(stale.clone());
        albums.push(keep.clone());
    }
    {
        let mut jobs = state.download_jobs.write().await;
        jobs.push(stale_job);
        jobs.push(keep_job);
    }

    services::sync_artist_albums(&state, a1.id)
        .await
        .expect("sync cleanup should not fail due to FK constraints");

    assert_no_foreign_key_violations(&state.db).await;

    let jobs = db::load_jobs(&state.db).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].album_id, keep.id);

    let albums = db::load_albums(&state.db).await.unwrap();
    assert!(
        albums.iter().any(|a| a.id == keep.id),
        "unrelated album should remain"
    );
    assert!(
        albums.iter().all(|a| a.id != stale.id),
        "stale album should be removed"
    );
}
