use std::sync::Arc;

use crate::db;
use crate::models::DownloadStatus;
use crate::providers::registry::ProviderRegistry;
use crate::providers::{ProviderAlbum, ProviderAlbumArtist, ProviderTrack};
use crate::test_helpers::*;
use yoink_shared::ServerAction;

use super::dispatch_action_impl;
use super::helpers::default_provider_artist_url;

// ── ToggleAlbumMonitor ──────────────────────────────────────

#[tokio::test]
async fn toggle_album_monitor_sets_flags() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let album = seed_album(&state.db, artist.id, "Album").await;

    state.monitored_artists.write().await.push(artist.clone());
    state.monitored_albums.write().await.push(album.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::ToggleAlbumMonitor {
            album_id: album.id,
            monitored: false,
        },
    )
    .await
    .unwrap();

    let albums = state.monitored_albums.read().await;
    let a = albums.iter().find(|a| a.id == album.id).unwrap();
    assert!(!a.monitored);
    assert!(!a.wanted);
}

#[tokio::test]
async fn toggle_album_monitor_enqueues_download_when_monitored() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let mut album = seed_album(&state.db, artist.id, "Album").await;
    album.monitored = false;
    album.wanted = false;
    album.acquired = false;
    db::upsert_album(&state.db, &album).await.unwrap();

    state.monitored_artists.write().await.push(artist.clone());
    state.monitored_albums.write().await.push(album.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::ToggleAlbumMonitor {
            album_id: album.id,
            monitored: true,
        },
    )
    .await
    .unwrap();

    let albums = state.monitored_albums.read().await;
    let a = albums.iter().find(|a| a.id == album.id).unwrap();
    assert!(a.monitored);

    let jobs = state.download_jobs.read().await;
    assert!(
        jobs.iter().any(|j| j.album_id == album.id),
        "should have enqueued a download job"
    );
}

// ── BulkMonitor ─────────────────────────────────────────────

#[tokio::test]
async fn bulk_monitor_toggles_all_albums_for_artist() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let a1 = seed_album(&state.db, artist.id, "Album 1").await;
    let a2 = seed_album(&state.db, artist.id, "Album 2").await;

    state.monitored_artists.write().await.push(artist.clone());
    {
        let mut albums = state.monitored_albums.write().await;
        albums.push(a1.clone());
        albums.push(a2.clone());
    }

    dispatch_action_impl(
        state.clone(),
        ServerAction::BulkMonitor {
            artist_id: artist.id,
            monitored: false,
        },
    )
    .await
    .unwrap();

    let albums = state.monitored_albums.read().await;
    assert!(albums.iter().all(|a| !a.monitored));
}

// ── RemoveArtist ────────────────────────────────────────────

#[tokio::test]
async fn remove_artist_cascades() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Removable").await;
    let album = seed_album(&state.db, artist.id, "Album").await;
    seed_tracks(&state.db, album.id, 3).await;
    seed_artist_provider_link(&state.db, artist.id, "tidal", "T1").await;

    state.monitored_artists.write().await.push(artist.clone());
    state.monitored_albums.write().await.push(album.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::RemoveArtist {
            artist_id: artist.id,
            remove_files: false,
        },
    )
    .await
    .unwrap();

    assert!(state.monitored_artists.read().await.is_empty());
    assert!(state.monitored_albums.read().await.is_empty());
    assert!(db::load_artists(&state.db).await.unwrap().is_empty());
    assert!(db::load_albums(&state.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn remove_artist_detaches_from_multi_artist_album() {
    let (state, _tmp) = test_app_state().await;
    let a1 = seed_artist(&state.db, "Artist 1").await;
    let a2 = seed_artist(&state.db, "Artist 2").await;

    let mut album = seed_album(&state.db, a1.id, "Collab").await;
    album.artist_ids = vec![a1.id, a2.id];
    db::upsert_album(&state.db, &album).await.unwrap();
    db::add_album_artist(&state.db, album.id, a2.id)
        .await
        .unwrap();

    state.monitored_artists.write().await.push(a1.clone());
    state.monitored_artists.write().await.push(a2.clone());
    state.monitored_albums.write().await.push(album.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::RemoveArtist {
            artist_id: a1.id,
            remove_files: false,
        },
    )
    .await
    .unwrap();

    let albums = state.monitored_albums.read().await;
    assert_eq!(albums.len(), 1);
    assert!(!albums[0].artist_ids.contains(&a1.id));
    assert_eq!(albums[0].artist_id, a2.id);
}

// ── CancelDownload ──────────────────────────────────────────

#[tokio::test]
async fn cancel_download_marks_job_failed() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let album = seed_album(&state.db, artist.id, "Album").await;
    let job = seed_job(&state.db, album.id, DownloadStatus::Queued).await;

    state.download_jobs.write().await.push(job.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::CancelDownload { job_id: job.id },
    )
    .await
    .unwrap();

    let jobs = state.download_jobs.read().await;
    let j = jobs.iter().find(|j| j.id == job.id).unwrap();
    assert!(matches!(j.status, DownloadStatus::Failed));
    assert_eq!(j.error.as_deref(), Some("Cancelled by user"));
}

#[tokio::test]
async fn cancel_download_ignores_non_queued() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let album = seed_album(&state.db, artist.id, "Album").await;
    let job = seed_job(&state.db, album.id, DownloadStatus::Downloading).await;

    state.download_jobs.write().await.push(job.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::CancelDownload { job_id: job.id },
    )
    .await
    .unwrap();

    let jobs = state.download_jobs.read().await;
    let j = jobs.iter().find(|j| j.id == job.id).unwrap();
    assert!(matches!(j.status, DownloadStatus::Downloading));
}

// ── ClearCompleted ──────────────────────────────────────────

#[tokio::test]
async fn clear_completed_removes_only_completed_jobs() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let album = seed_album(&state.db, artist.id, "Album").await;

    let j1 = seed_job(&state.db, album.id, DownloadStatus::Completed).await;
    let j2 = seed_job(&state.db, album.id, DownloadStatus::Queued).await;
    let j3 = seed_job(&state.db, album.id, DownloadStatus::Completed).await;

    {
        let mut jobs = state.download_jobs.write().await;
        jobs.push(j1);
        jobs.push(j2.clone());
        jobs.push(j3);
    }

    dispatch_action_impl(state.clone(), ServerAction::ClearCompleted)
        .await
        .unwrap();

    let jobs = state.download_jobs.read().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, j2.id);
}

// ── UpdateArtist ────────────────────────────────────────────

#[tokio::test]
async fn update_artist_name_and_image() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Old Name").await;
    state.monitored_artists.write().await.push(artist.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::UpdateArtist {
            artist_id: artist.id,
            name: Some("New Name".to_string()),
            image_url: Some("https://img.test/photo.jpg".to_string()),
        },
    )
    .await
    .unwrap();

    let artists = state.monitored_artists.read().await;
    let a = artists.iter().find(|a| a.id == artist.id).unwrap();
    assert_eq!(a.name, "New Name");
    assert_eq!(a.image_url.as_deref(), Some("https://img.test/photo.jpg"));

    let db_artists = db::load_artists(&state.db).await.unwrap();
    assert_eq!(db_artists[0].name, "New Name");
}

#[tokio::test]
async fn update_artist_clear_image_with_empty_string() {
    let (state, _tmp) = test_app_state().await;
    let mut artist = seed_artist(&state.db, "Artist").await;
    artist.image_url = Some("https://old.test/img.jpg".to_string());
    db::upsert_artist(&state.db, &artist).await.unwrap();
    state.monitored_artists.write().await.push(artist.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::UpdateArtist {
            artist_id: artist.id,
            name: None,
            image_url: Some(String::new()),
        },
    )
    .await
    .unwrap();

    let artists = state.monitored_artists.read().await;
    let a = artists.iter().find(|a| a.id == artist.id).unwrap();
    assert!(a.image_url.is_none());
}

// ── LinkArtistProvider / UnlinkArtistProvider ────────────────

#[tokio::test]
async fn link_and_unlink_artist_provider() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    state.monitored_artists.write().await.push(artist.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::LinkArtistProvider {
            artist_id: artist.id,
            provider: "deezer".to_string(),
            external_id: "D999".to_string(),
            external_url: None,
            external_name: Some("Deezer Artist".to_string()),
            image_ref: None,
        },
    )
    .await
    .unwrap();

    let links = db::load_artist_provider_links(&state.db, artist.id)
        .await
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].provider, "deezer");
    assert_eq!(links[0].external_id, "D999");
    assert_eq!(
        links[0].external_url.as_deref(),
        Some("https://www.deezer.com/artist/D999")
    );

    dispatch_action_impl(
        state.clone(),
        ServerAction::UnlinkArtistProvider {
            artist_id: artist.id,
            provider: "deezer".to_string(),
            external_id: "D999".to_string(),
        },
    )
    .await
    .unwrap();

    let links = db::load_artist_provider_links(&state.db, artist.id)
        .await
        .unwrap();
    assert!(links.is_empty());
}

// ── AddAlbumArtist / RemoveAlbumArtist ──────────────────────

#[tokio::test]
async fn add_and_remove_album_artist() {
    let (state, _tmp) = test_app_state().await;
    let a1 = seed_artist(&state.db, "Artist 1").await;
    let a2 = seed_artist(&state.db, "Artist 2").await;
    let album = seed_album(&state.db, a1.id, "Album").await;

    state.monitored_artists.write().await.push(a1.clone());
    state.monitored_artists.write().await.push(a2.clone());
    state.monitored_albums.write().await.push(album.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::AddAlbumArtist {
            album_id: album.id,
            artist_id: a2.id,
        },
    )
    .await
    .unwrap();

    {
        let albums = state.monitored_albums.read().await;
        let a = albums.iter().find(|a| a.id == album.id).unwrap();
        assert_eq!(a.artist_ids.len(), 2);
        assert!(a.artist_ids.contains(&a2.id));
    }

    dispatch_action_impl(
        state.clone(),
        ServerAction::RemoveAlbumArtist {
            album_id: album.id,
            artist_id: a1.id,
        },
    )
    .await
    .unwrap();

    {
        let albums = state.monitored_albums.read().await;
        let a = albums.iter().find(|a| a.id == album.id).unwrap();
        assert_eq!(a.artist_ids, vec![a2.id]);
        assert_eq!(a.artist_id, a2.id);
    }
}

#[tokio::test]
async fn remove_sole_album_artist_returns_error() {
    let (state, _tmp) = test_app_state().await;
    let a1 = seed_artist(&state.db, "Solo").await;
    let album = seed_album(&state.db, a1.id, "Album").await;

    state.monitored_albums.write().await.push(album.clone());

    let result = dispatch_action_impl(
        state.clone(),
        ServerAction::RemoveAlbumArtist {
            album_id: album.id,
            artist_id: a1.id,
        },
    )
    .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .contains("Cannot remove the only artist"));
}

// ── MergeAlbums ─────────────────────────────────────────────

#[tokio::test]
async fn merge_albums_combines_tracks_and_links() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let target = seed_album(&state.db, artist.id, "Target Album").await;
    let source = seed_album(&state.db, artist.id, "Source Album").await;
    seed_tracks(&state.db, target.id, 2).await;
    seed_tracks(&state.db, source.id, 3).await;
    seed_album_provider_link(&state.db, source.id, "tidal", "SRC1").await;

    state.monitored_artists.write().await.push(artist.clone());
    {
        let mut albums = state.monitored_albums.write().await;
        albums.push(target.clone());
        albums.push(source.clone());
    }

    dispatch_action_impl(
        state.clone(),
        ServerAction::MergeAlbums {
            target_album_id: target.id,
            source_album_id: source.id,
            result_title: Some("Merged Album".to_string()),
            result_cover_url: None,
        },
    )
    .await
    .unwrap();

    let albums = state.monitored_albums.read().await;
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].id, target.id);
    assert_eq!(albums[0].title, "Merged Album");

    let tracks = db::load_tracks_for_album(&state.db, target.id)
        .await
        .unwrap();
    assert_eq!(tracks.len(), 5);

    let links = db::load_album_provider_links(&state.db, target.id)
        .await
        .unwrap();
    assert!(links.iter().any(|l| l.external_id == "SRC1"));
}

#[tokio::test]
async fn merge_albums_same_id_returns_error() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let album = seed_album(&state.db, artist.id, "Album").await;

    state.monitored_albums.write().await.push(album.clone());

    let result = dispatch_action_impl(
        state.clone(),
        ServerAction::MergeAlbums {
            target_album_id: album.id,
            source_album_id: album.id,
            result_title: None,
            result_cover_url: None,
        },
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("must be different"));
}

// ── ToggleTrackMonitor ──────────────────────────────────────

#[tokio::test]
async fn toggle_track_monitor_updates_flags_and_partially_wanted() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    let mut album = seed_album(&state.db, artist.id, "Album").await;
    album.monitored = false;
    album.wanted = false;
    db::upsert_album(&state.db, &album).await.unwrap();

    let tracks = seed_tracks(&state.db, album.id, 2).await;

    state.monitored_artists.write().await.push(artist.clone());
    state.monitored_albums.write().await.push(album.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::ToggleTrackMonitor {
            track_id: tracks[0].id,
            album_id: album.id,
            monitored: true,
        },
    )
    .await
    .unwrap();

    let db_tracks = db::load_tracks_for_album(&state.db, album.id)
        .await
        .unwrap();
    let t = db_tracks.iter().find(|t| t.id == tracks[0].id).unwrap();
    assert!(t.monitored);

    let albums = state.monitored_albums.read().await;
    let a = albums.iter().find(|a| a.id == album.id).unwrap();
    assert!(a.partially_wanted);
}

// ── AddArtist with mock provider ────────────────────────────

#[tokio::test]
async fn add_artist_creates_artist_and_provider_link() {
    let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
    *mock.fetch_albums_result.lock().await = Ok(vec![]);

    let mut registry = ProviderRegistry::new();
    registry.register_metadata(mock as Arc<dyn crate::providers::MetadataProvider>);

    let (state, _tmp) = test_app_state_with_registry(registry).await;

    dispatch_action_impl(
        state.clone(),
        ServerAction::AddArtist {
            name: "New Artist".to_string(),
            provider: "mock_prov".to_string(),
            external_id: "EXT_NEW".to_string(),
            image_url: Some("https://img.test/new.jpg".to_string()),
            external_url: None,
        },
    )
    .await
    .unwrap();

    let artists = state.monitored_artists.read().await;
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].name, "New Artist");
    assert!(artists[0].monitored);

    let artist_id = artists[0].id;
    let links = db::load_artist_provider_links(&state.db, artist_id)
        .await
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].provider, "mock_prov");
    assert_eq!(links[0].external_id, "EXT_NEW");
}

#[tokio::test]
async fn add_artist_reuses_existing_via_provider_link() {
    let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
    *mock.fetch_albums_result.lock().await = Ok(vec![]);

    let mut registry = ProviderRegistry::new();
    registry.register_metadata(mock as Arc<dyn crate::providers::MetadataProvider>);

    let (state, _tmp) = test_app_state_with_registry(registry).await;

    let artist = seed_artist(&state.db, "Existing").await;
    seed_artist_provider_link(&state.db, artist.id, "mock_prov", "EXIST_1").await;
    state.monitored_artists.write().await.push(artist.clone());

    dispatch_action_impl(
        state.clone(),
        ServerAction::AddArtist {
            name: "Existing".to_string(),
            provider: "mock_prov".to_string(),
            external_id: "EXIST_1".to_string(),
            image_url: None,
            external_url: None,
        },
    )
    .await
    .unwrap();

    let artists = state.monitored_artists.read().await;
    assert_eq!(artists.len(), 1);
}

// ── AddAlbum with mock provider ─────────────────────────────

#[tokio::test]
async fn add_album_creates_album_and_tracks() {
    let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
    *mock.fetch_albums_result.lock().await = Ok(vec![ProviderAlbum {
        external_id: "ALB_EXT".to_string(),
        title: "Mock Album".to_string(),
        album_type: Some("album".to_string()),
        release_date: Some("2024-06-01".to_string()),
        cover_ref: None,
        url: None,
        explicit: false,
        artists: vec![ProviderAlbumArtist {
            external_id: "ART_EXT".to_string(),
            name: "Mock Artist".to_string(),
        }],
    }]);
    *mock.fetch_tracks_result.lock().await = Ok((
        vec![
            ProviderTrack {
                external_id: "TRK1".to_string(),
                title: "Track One".to_string(),
                version: None,
                track_number: 1,
                disc_number: Some(1),
                duration_secs: 200,
                isrc: Some("US1234567890".to_string()),
                artists: None,
                explicit: false,
                extra: std::collections::HashMap::new(),
            },
            ProviderTrack {
                external_id: "TRK2".to_string(),
                title: "Track Two".to_string(),
                version: None,
                track_number: 2,
                disc_number: Some(1),
                duration_secs: 180,
                isrc: None,
                artists: None,
                explicit: false,
                extra: std::collections::HashMap::new(),
            },
        ],
        std::collections::HashMap::new(),
    ));

    let mut registry = ProviderRegistry::new();
    registry.register_metadata(mock as Arc<dyn crate::providers::MetadataProvider>);

    let (state, _tmp) = test_app_state_with_registry(registry).await;

    dispatch_action_impl(
        state.clone(),
        ServerAction::AddAlbum {
            provider: "mock_prov".to_string(),
            external_album_id: "ALB_EXT".to_string(),
            artist_external_id: "ART_EXT".to_string(),
            artist_name: "Mock Artist".to_string(),
            monitor_all: true,
        },
    )
    .await
    .unwrap();

    let artists = state.monitored_artists.read().await;
    assert_eq!(artists.len(), 1);
    assert!(!artists[0].monitored); // lightweight

    let albums = state.monitored_albums.read().await;
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].title, "Mock Album");
    assert!(albums[0].monitored);
    let album_id = albums[0].id;
    drop(albums);

    let tracks = db::load_tracks_for_album(&state.db, album_id)
        .await
        .unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].title, "Track One");
    assert!(tracks[0].monitored);

    let album_links = db::load_album_provider_links(&state.db, album_id)
        .await
        .unwrap();
    assert_eq!(album_links.len(), 1);
    assert_eq!(album_links[0].external_id, "ALB_EXT");
}

// ── DismissMatchSuggestion ──────────────────────────────────

#[tokio::test]
async fn dismiss_match_suggestion() {
    let (state, _tmp) = test_app_state().await;
    let artist = seed_artist(&state.db, "Artist").await;
    state.monitored_artists.write().await.push(artist.clone());

    let suggestion = db::MatchSuggestion {
        id: uuid::Uuid::now_v7(),
        scope_type: "artist".to_string(),
        scope_id: artist.id,
        left_provider: "tidal".to_string(),
        left_external_id: "T1".to_string(),
        right_provider: "deezer".to_string(),
        right_external_id: "D1".to_string(),
        match_kind: "name_match".to_string(),
        confidence: 80,
        explanation: None,
        external_name: None,
        external_url: None,
        image_ref: None,
        disambiguation: None,
        artist_type: None,
        country: None,
        tags: vec![],
        popularity: None,
        status: "pending".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::upsert_match_suggestion(&state.db, &suggestion)
        .await
        .unwrap();

    dispatch_action_impl(
        state.clone(),
        ServerAction::DismissMatchSuggestion {
            suggestion_id: suggestion.id,
        },
    )
    .await
    .unwrap();

    let loaded = db::load_match_suggestion_by_id(&state.db, suggestion.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, "dismissed");
}

// ── default_provider_artist_url ─────────────────────────────

#[test]
fn default_provider_artist_url_known_providers() {
    assert_eq!(
        default_provider_artist_url("tidal", "123"),
        Some("https://tidal.com/browse/artist/123".to_string())
    );
    assert_eq!(
        default_provider_artist_url("deezer", "456"),
        Some("https://www.deezer.com/artist/456".to_string())
    );
    assert_eq!(
        default_provider_artist_url("musicbrainz", "abc"),
        Some("https://musicbrainz.org/artist/abc".to_string())
    );
    assert!(default_provider_artist_url("unknown", "x").is_none());
}
