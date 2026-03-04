use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::{db, services, state::AppState};

use super::helpers;

pub(super) async fn add_track(
    state: &AppState,
    provider: String,
    external_track_id: String,
    external_album_id: String,
    artist_external_id: String,
    artist_name: String,
) -> Result<(), String> {
    // 1. Find or create lightweight (unmonitored) artist.
    let artist_id = helpers::find_or_create_lightweight_artist(
        state,
        &provider,
        &artist_external_id,
        &artist_name,
    )
    .await?;

    // 2. Fetch album metadata to create the parent album.
    let prov = state
        .registry
        .metadata_provider(&provider)
        .ok_or_else(|| format!("Unknown metadata provider: {provider}"))?;

    let albums = prov
        .fetch_albums(&artist_external_id)
        .await
        .map_err(|e| format!("Failed to fetch albums: {}", e.0))?;

    let prov_album = albums
        .into_iter()
        .find(|a| a.external_id == external_album_id)
        .ok_or_else(|| "Album not found in provider's album listing".to_string())?;

    // 3. Find or create the album.
    let existing_album_id =
        db::find_album_by_provider_link(&state.db, &provider, &external_album_id)
            .await
            .ok()
            .flatten();

    let album_id = if let Some(id) = existing_album_id {
        id
    } else {
        let new_id = Uuid::now_v7();
        let album = yoink_shared::MonitoredAlbum {
            id: new_id,
            artist_id,
            artist_ids: vec![artist_id],
            artist_credits: prov_album
                .artists
                .iter()
                .map(|a| yoink_shared::ArtistCredit {
                    name: a.name.clone(),
                    provider: Some(provider.clone()),
                    external_id: Some(a.external_id.clone()),
                })
                .collect(),
            title: prov_album.title.clone(),
            album_type: prov_album.album_type.clone(),
            release_date: prov_album.release_date.clone(),
            cover_url: prov_album
                .cover_ref
                .as_deref()
                .map(|c| yoink_shared::provider_image_url(&provider, c, 640)),
            explicit: prov_album.explicit,
            monitored: false, // album-level not monitored; only the specific track
            acquired: false,
            wanted: false,
            partially_wanted: true, // will have a monitored track
            added_at: Utc::now(),
        };
        let _ = db::upsert_album(&state.db, &album).await;

        let link = db::AlbumProviderLink {
            id: Uuid::now_v7(),
            album_id: new_id,
            provider: provider.clone(),
            external_id: external_album_id.clone(),
            external_url: prov_album.url.clone(),
            external_title: Some(prov_album.title.clone()),
            cover_ref: prov_album.cover_ref.clone(),
        };
        let _ = db::upsert_album_provider_link(&state.db, &link).await;
        let _ = db::add_album_artist(&state.db, new_id, artist_id).await;

        {
            let mut albums = state.monitored_albums.write().await;
            albums.push(album);
        }
        new_id
    };

    // 4. Fetch and store tracks (none monitored by default).
    helpers::store_album_tracks(state, &provider, &external_album_id, album_id, false).await?;

    // 5. Find the target track and mark it as monitored.
    if let Ok(Some(track_id)) =
        db::find_track_by_provider_link(&state.db, &provider, &external_track_id).await
    {
        let _ = db::update_track_flags(&state.db, track_id, true, false).await;

        // Recompute partially_wanted
        {
            let mut albums = state.monitored_albums.write().await;
            if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
                services::recompute_partially_wanted(&state.db, album).await;
                if album.partially_wanted {
                    let album_clone = album.clone();
                    drop(albums);
                    services::enqueue_album_download(state, &album_clone).await;
                }
            }
        }
    }

    info!(%album_id, %provider, %external_track_id, "Added track from search");
    state.notify_sse();
    Ok(())
}

pub(super) async fn toggle_track_monitor(
    state: &AppState,
    track_id: Uuid,
    album_id: Uuid,
    monitored: bool,
) -> Result<(), String> {
    // Update the track's monitored flag in DB
    let current_acquired = {
        let tracks = db::load_tracks_for_album(&state.db, album_id)
            .await
            .unwrap_or_default();
        tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.acquired)
            .unwrap_or(false)
    };
    let _ = db::update_track_flags(&state.db, track_id, monitored, current_acquired).await;

    // Recompute the album's partially_wanted flag
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
            services::recompute_partially_wanted(&state.db, album).await;
            // If track became wanted, trigger album download (the download
            // worker will handle track-level filtering in Phase 3)
            if monitored && !current_acquired && (album.wanted || album.partially_wanted) {
                let album_clone = album.clone();
                drop(albums);
                services::enqueue_album_download(state, &album_clone).await;
            }
        }
    }
    info!(%track_id, %album_id, monitored, "Toggled track monitored status");
    state.notify_sse();
    Ok(())
}

pub(super) async fn bulk_toggle_track_monitor(
    state: &AppState,
    album_id: Uuid,
    monitored: bool,
) -> Result<(), String> {
    // Update all tracks for the album
    let tracks = db::load_tracks_for_album(&state.db, album_id)
        .await
        .unwrap_or_default();

    for track in &tracks {
        let _ = db::update_track_flags(&state.db, track.id, monitored, track.acquired).await;
    }

    // Recompute the album's partially_wanted flag and potentially enqueue
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
            services::recompute_partially_wanted(&state.db, album).await;
            if album.wanted || album.partially_wanted {
                let album_clone = album.clone();
                drop(albums);
                services::enqueue_album_download(state, &album_clone).await;
            }
        }
    }

    let count = tracks.len();
    info!(%album_id, monitored, count, "Bulk toggled track monitoring");
    state.notify_sse();
    Ok(())
}
