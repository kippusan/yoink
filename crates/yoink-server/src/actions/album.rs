use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::{db, services, state::AppState};

use super::helpers;

pub(super) async fn toggle_album_monitor(
    state: &AppState,
    album_id: Uuid,
    monitored: bool,
) -> Result<(), String> {
    let mut album_to_queue = None;
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
            album.monitored = monitored;
            services::update_wanted(album);
            let _ = db::update_album_flags(
                &state.db,
                album.id,
                album.monitored,
                album.acquired,
                album.wanted,
            )
            .await;
            if album.monitored && !album.acquired {
                album_to_queue = Some(album.clone());
            }
        }
    }
    if let Some(album) = album_to_queue {
        services::enqueue_album_download(state, &album).await;
    }
    state.notify_sse();
    Ok(())
}

pub(super) async fn bulk_monitor(
    state: &AppState,
    artist_id: Uuid,
    monitored: bool,
) -> Result<(), String> {
    let mut to_queue = Vec::new();
    {
        let mut albums = state.monitored_albums.write().await;
        for album in albums
            .iter_mut()
            .filter(|a| a.artist_id == artist_id || a.artist_ids.contains(&artist_id))
        {
            album.monitored = monitored;
            services::update_wanted(album);
            let _ = db::update_album_flags(
                &state.db,
                album.id,
                album.monitored,
                album.acquired,
                album.wanted,
            )
            .await;
            if album.monitored && !album.acquired {
                to_queue.push(album.clone());
            }
        }
    }
    for album in to_queue {
        services::enqueue_album_download(state, &album).await;
    }
    state.notify_sse();
    Ok(())
}

pub(super) async fn merge_albums(
    state: &AppState,
    target_album_id: Uuid,
    source_album_id: Uuid,
    result_title: Option<String>,
    result_cover_url: Option<String>,
) -> Result<(), String> {
    services::merge_albums(
        state,
        target_album_id,
        source_album_id,
        result_title.as_deref(),
        result_cover_url.as_deref(),
    )
    .await?;

    let artist_id = {
        let albums = state.monitored_albums.read().await;
        albums
            .iter()
            .find(|a| a.id == target_album_id)
            .map(|a| a.artist_id)
    };
    if let Some(artist_id) = artist_id {
        helpers::spawn_recompute_artist_match_suggestions(state, artist_id);
    }
    state.notify_sse();
    Ok(())
}

pub(super) async fn remove_album_files(
    state: &AppState,
    album_id: Uuid,
    unmonitor: bool,
) -> Result<(), String> {
    let album = {
        let albums = state.monitored_albums.read().await;
        albums.iter().find(|a| a.id == album_id).cloned()
    }
    .ok_or_else(|| format!("album {} not found", album_id))?;

    let removed = services::remove_downloaded_album_files(state, &album).await?;

    let mut to_queue = None;
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(existing) = albums.iter_mut().find(|a| a.id == album_id) {
            existing.acquired = false;
            if unmonitor {
                existing.monitored = false;
            }
            services::update_wanted(existing);
            let _ = db::update_album_flags(
                &state.db,
                existing.id,
                existing.monitored,
                existing.acquired,
                existing.wanted,
            )
            .await;
            if existing.monitored {
                to_queue = Some(existing.clone());
            }
        }
    }

    let mut removed_completed_ids = Vec::new();
    {
        let mut jobs = state.download_jobs.write().await;
        jobs.retain(|j| {
            let should_remove =
                j.album_id == album_id && j.status == yoink_shared::DownloadStatus::Completed;
            if should_remove {
                removed_completed_ids.push(j.id);
            }
            !should_remove
        });
    }
    for job_id in removed_completed_ids {
        let _ = db::delete_job(&state.db, job_id).await;
    }

    if let Some(album) = to_queue {
        services::enqueue_album_download(state, &album).await;
    }

    info!(
        %album_id,
        removed, unmonitor, "Removed downloaded album files"
    );
    state.notify_sse();
    Ok(())
}

pub(super) async fn add_album_artist(
    state: &AppState,
    album_id: Uuid,
    artist_id: Uuid,
) -> Result<(), String> {
    db::add_album_artist(&state.db, album_id, artist_id)
        .await
        .map_err(|e| format!("failed to add album artist: {e}"))?;
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|a| a.id == album_id)
            && !album.artist_ids.contains(&artist_id)
        {
            album.artist_ids.push(artist_id);
        }
    }
    state.notify_sse();
    Ok(())
}

pub(super) async fn remove_album_artist(
    state: &AppState,
    album_id: Uuid,
    artist_id: Uuid,
) -> Result<(), String> {
    // Must keep at least one artist
    {
        let albums = state.monitored_albums.read().await;
        if let Some(album) = albums.iter().find(|a| a.id == album_id)
            && album.artist_ids.len() <= 1
        {
            return Err("Cannot remove the only artist from an album".to_string());
        }
    }
    db::remove_album_artist(&state.db, album_id, artist_id)
        .await
        .map_err(|e| format!("failed to remove album artist: {e}"))?;
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
            album.artist_ids.retain(|id| *id != artist_id);
            if album.artist_id == artist_id && !album.artist_ids.is_empty() {
                album.artist_id = album.artist_ids[0];
                // Update the legacy column
                let _ = db::upsert_album(&state.db, album).await;
            }
        }
    }
    state.notify_sse();
    Ok(())
}

pub(super) async fn add_album(
    state: &AppState,
    provider: String,
    external_album_id: String,
    artist_external_id: String,
    artist_name: String,
    monitor_all: bool,
) -> Result<(), String> {
    // 1. Find or create lightweight (unmonitored) artist.
    let artist_id = helpers::find_or_create_lightweight_artist(
        state,
        &provider,
        &artist_external_id,
        &artist_name,
    )
    .await?;

    // 2. Fetch album metadata from the provider.
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

    // 3. Check if album already exists via provider link.
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
            monitored: monitor_all,
            acquired: false,
            wanted: monitor_all,
            partially_wanted: false,
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

    // 4. Fetch and store tracks.
    helpers::store_album_tracks(state, &provider, &external_album_id, album_id, monitor_all)
        .await?;

    // 5. If monitored, queue download.
    if monitor_all {
        let album = {
            let albums = state.monitored_albums.read().await;
            albums.iter().find(|a| a.id == album_id).cloned()
        };
        if let Some(album) = album {
            services::enqueue_album_download(state, &album).await;
        }
    }

    info!(%album_id, %provider, %external_album_id, monitor_all, "Added album from search");
    state.notify_sse();
    Ok(())
}
