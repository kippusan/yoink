use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::{db, services, state::AppState};

use super::helpers;

pub(super) async fn add_artist(
    state: &AppState,
    name: String,
    provider: String,
    external_id: String,
    image_url: Option<String>,
    external_url: Option<String>,
) -> Result<(), String> {
    // Generate a default external URL if none was provided
    let external_url =
        external_url.or_else(|| helpers::default_provider_artist_url(&provider, &external_id));

    let existing_artist_id =
        db::find_artist_by_provider_link(&state.db, &provider, &external_id)
            .await
            .ok()
            .flatten();

    let artist_id = if let Some(id) = existing_artist_id {
        id
    } else {
        let new_id = Uuid::now_v7();
        let artist = yoink_shared::MonitoredArtist {
            id: new_id,
            name: name.clone(),
            image_url: image_url.clone(),
            bio: None,
            monitored: true, // Artists added via search are fully monitored
            added_at: Utc::now(),
        };
        let _ = db::upsert_artist(&state.db, &artist).await;
        {
            let mut artists = state.monitored_artists.write().await;
            artists.push(artist);
        }

        let link = db::ArtistProviderLink {
            id: Uuid::now_v7(),
            artist_id: new_id,
            provider: provider.clone(),
            external_id: external_id.clone(),
            external_url: external_url.clone(),
            external_name: Some(name),
            image_ref: None,
        };
        let _ = db::upsert_artist_provider_link(&state.db, &link).await;

        new_id
    };

    let _ = services::sync_artist_albums(state, artist_id).await;
    helpers::spawn_fetch_artist_bio(state, artist_id);
    helpers::spawn_recompute_artist_match_suggestions(state, artist_id);
    state.notify_sse();
    Ok(())
}

pub(super) async fn remove_artist(
    state: &AppState,
    artist_id: Uuid,
    remove_files: bool,
) -> Result<(), String> {
    use tracing::warn;

    if remove_files {
        let acquired: Vec<_> = {
            let albums = state.monitored_albums.read().await;
            albums
                .iter()
                .filter(|a| {
                    (a.artist_id == artist_id || a.artist_ids.contains(&artist_id)) && a.acquired
                })
                .cloned()
                .collect()
        };
        for album in &acquired {
            if let Err(e) = services::remove_downloaded_album_files(state, album).await {
                warn!(
                    album_id = %album.id,
                    error = %e,
                    "Failed to remove files for album while removing artist"
                );
            }
        }
    }
    // Remove albums solely owned by this artist; for multi-artist albums
    // just detach this artist.
    {
        let mut albums = state.monitored_albums.write().await;
        let mut sole_album_ids = Vec::new();
        for album in albums.iter_mut() {
            let is_related =
                album.artist_id == artist_id || album.artist_ids.contains(&artist_id);
            if !is_related {
                continue;
            }
            if album.artist_ids.len() <= 1 {
                // Sole artist — delete the album entirely
                sole_album_ids.push(album.id);
            } else {
                // Multi-artist — remove this artist from the list
                album.artist_ids.retain(|id| *id != artist_id);
                if album.artist_id == artist_id {
                    album.artist_id = album.artist_ids[0];
                }
                let _ = db::upsert_album(&state.db, album).await;
                let _ = db::remove_album_artist(&state.db, album.id, artist_id).await;
            }
        }
        for id in &sole_album_ids {
            let _ = db::delete_album(&state.db, *id).await;
        }
        albums.retain(|a| !sole_album_ids.contains(&a.id));
    }
    let _ = db::delete_albums_by_artist(&state.db, artist_id).await;
    let _ = db::delete_album_artists_by_artist(&state.db, artist_id).await;
    let _ = db::delete_artist(&state.db, artist_id).await;
    {
        let mut artists = state.monitored_artists.write().await;
        artists.retain(|a| a.id != artist_id);
    }
    info!(%artist_id, remove_files, "Removed artist and their albums");
    state.notify_sse();
    Ok(())
}

pub(super) async fn update_artist(
    state: &AppState,
    artist_id: Uuid,
    name: Option<String>,
    image_url: Option<String>,
) -> Result<(), String> {
    let db_name: Option<&str> = name.as_deref();
    // Empty string means "clear image"
    let db_image: Option<Option<&str>> = image_url
        .as_ref()
        .map(|u| if u.is_empty() { None } else { Some(u.as_str()) });
    db::update_artist_details(&state.db, artist_id, db_name, db_image)
        .await
        .map_err(|e| format!("failed to update artist: {e}"))?;
    {
        let mut artists = state.monitored_artists.write().await;
        if let Some(a) = artists.iter_mut().find(|a| a.id == artist_id) {
            if let Some(ref new_name) = name {
                a.name = new_name.clone();
            }
            if let Some(ref new_url) = image_url {
                a.image_url = if new_url.is_empty() {
                    None
                } else {
                    Some(new_url.clone())
                };
            }
        }
    }
    info!(%artist_id, ?name, ?image_url, "Updated artist details");
    state.notify_sse();
    Ok(())
}

pub(super) async fn toggle_artist_monitor(
    state: &AppState,
    artist_id: Uuid,
    monitored: bool,
) -> Result<(), String> {
    let _ = db::update_artist_monitored(&state.db, artist_id, monitored).await;
    {
        let mut artists = state.monitored_artists.write().await;
        if let Some(artist) = artists.iter_mut().find(|a| a.id == artist_id) {
            artist.monitored = monitored;
        }
    }
    if monitored {
        // Promoting to fully monitored — sync discography from providers
        let _ = services::sync_artist_albums(state, artist_id).await;
        helpers::spawn_fetch_artist_bio(state, artist_id);
        helpers::spawn_recompute_artist_match_suggestions(state, artist_id);
    }
    info!(%artist_id, monitored, "Toggled artist monitored status");
    state.notify_sse();
    Ok(())
}

pub(super) async fn fetch_artist_bio(state: &AppState, artist_id: Uuid) -> Result<(), String> {
    info!(%artist_id, "Manual bio fetch requested, clearing existing bio");
    // Clear old bio first so the fetch replaces it
    let _ = db::update_artist_bio(&state.db, artist_id, None).await;
    {
        let mut artists = state.monitored_artists.write().await;
        if let Some(a) = artists.iter_mut().find(|a| a.id == artist_id) {
            a.bio = None;
        }
    }
    state.notify_sse();
    helpers::spawn_fetch_artist_bio(state, artist_id);
    Ok(())
}

pub(super) async fn sync_artist_albums(
    state: &AppState,
    artist_id: Uuid,
) -> Result<(), String> {
    let _ = services::sync_artist_albums(state, artist_id).await;
    {
        let artists = state.monitored_artists.read().await;
        let has_bio = artists
            .iter()
            .find(|a| a.id == artist_id)
            .map(|a| a.bio.is_some())
            .unwrap_or(false);
        if !has_bio {
            helpers::spawn_fetch_artist_bio(state, artist_id);
        }
    }
    helpers::spawn_recompute_artist_match_suggestions(state, artist_id);
    state.notify_sse();
    Ok(())
}

pub(super) async fn link_artist_provider(
    state: &AppState,
    artist_id: Uuid,
    provider: String,
    external_id: String,
    external_url: Option<String>,
    external_name: Option<String>,
    image_ref: Option<String>,
) -> Result<(), String> {
    let external_url =
        external_url.or_else(|| helpers::default_provider_artist_url(&provider, &external_id));
    let link = db::ArtistProviderLink {
        id: Uuid::now_v7(),
        artist_id,
        provider,
        external_id,
        external_url,
        external_name,
        image_ref,
    };
    let _ = db::upsert_artist_provider_link(&state.db, &link).await;
    {
        let artists = state.monitored_artists.read().await;
        let has_bio = artists
            .iter()
            .find(|a| a.id == link.artist_id)
            .map(|a| a.bio.is_some())
            .unwrap_or(false);
        if !has_bio {
            helpers::spawn_fetch_artist_bio(state, link.artist_id);
        }
    }
    helpers::spawn_recompute_artist_match_suggestions(state, link.artist_id);
    state.notify_sse();
    Ok(())
}

pub(super) async fn unlink_artist_provider(
    state: &AppState,
    artist_id: Uuid,
    provider: String,
    external_id: String,
) -> Result<(), String> {
    let _ =
        db::delete_artist_provider_link(&state.db, artist_id, &provider, &external_id).await;
    helpers::spawn_recompute_artist_match_suggestions(state, artist_id);
    state.notify_sse();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::db;
    use crate::providers::registry::ProviderRegistry;
    use crate::test_helpers::*;

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

        super::remove_artist(&state, artist.id, false)
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

        super::remove_artist(&state, a1.id, false).await.unwrap();

        let albums = state.monitored_albums.read().await;
        assert_eq!(albums.len(), 1);
        assert!(!albums[0].artist_ids.contains(&a1.id));
        assert_eq!(albums[0].artist_id, a2.id);
    }

    // ── UpdateArtist ────────────────────────────────────────────

    #[tokio::test]
    async fn update_artist_name_and_image() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Old Name").await;
        state.monitored_artists.write().await.push(artist.clone());

        super::update_artist(
            &state,
            artist.id,
            Some("New Name".to_string()),
            Some("https://img.test/photo.jpg".to_string()),
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

        super::update_artist(&state, artist.id, None, Some(String::new()))
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

        super::link_artist_provider(
            &state,
            artist.id,
            "deezer".to_string(),
            "D999".to_string(),
            None,
            Some("Deezer Artist".to_string()),
            None,
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

        super::unlink_artist_provider(
            &state,
            artist.id,
            "deezer".to_string(),
            "D999".to_string(),
        )
        .await
        .unwrap();

        let links = db::load_artist_provider_links(&state.db, artist.id)
            .await
            .unwrap();
        assert!(links.is_empty());
    }

    // ── AddArtist with mock provider ────────────────────────────

    #[tokio::test]
    async fn add_artist_creates_artist_and_provider_link() {
        let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
        *mock.fetch_albums_result.lock().await = Ok(vec![]);

        let mut registry = ProviderRegistry::new();
        registry.register_metadata(mock as Arc<dyn crate::providers::MetadataProvider>);

        let (state, _tmp) = test_app_state_with_registry(registry).await;

        super::add_artist(
            &state,
            "New Artist".to_string(),
            "mock_prov".to_string(),
            "EXT_NEW".to_string(),
            Some("https://img.test/new.jpg".to_string()),
            None,
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

        super::add_artist(
            &state,
            "Existing".to_string(),
            "mock_prov".to_string(),
            "EXIST_1".to_string(),
            None,
            None,
        )
        .await
        .unwrap();

        let artists = state.monitored_artists.read().await;
        assert_eq!(artists.len(), 1);
    }
}
