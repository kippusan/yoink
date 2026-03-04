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
