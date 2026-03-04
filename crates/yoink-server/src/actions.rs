use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{db, services, state::AppState};

/// Generate a default external URL for an artist on a given provider.
fn default_provider_artist_url(provider: &str, external_id: &str) -> Option<String> {
    match provider {
        "tidal" => Some(format!("https://tidal.com/browse/artist/{external_id}")),
        "deezer" => Some(format!("https://www.deezer.com/artist/{external_id}")),
        "musicbrainz" => Some(format!("https://musicbrainz.org/artist/{external_id}")),
        _ => None,
    }
}

/// Fetch artist bio from linked metadata providers in background.
pub(crate) fn spawn_fetch_artist_bio(state: &AppState, artist_id: Uuid) {
    let s = state.clone();
    info!(%artist_id, "Spawning background bio fetch");
    tokio::spawn(async move {
        let links = match db::load_artist_provider_links(&s.db, artist_id).await {
            Ok(l) => l,
            Err(e) => {
                warn!(%artist_id, error = %e, "Failed to load provider links for bio fetch");
                return;
            }
        };

        if links.is_empty() {
            info!(%artist_id, "No provider links found, skipping bio fetch");
            return;
        }

        info!(%artist_id, link_count = links.len(), "Attempting bio fetch from linked providers");

        for link in &links {
            let provider_name = &link.provider;
            let external_id = &link.external_id;

            let Some(provider) = s.registry.metadata_provider(provider_name) else {
                info!(
                    %artist_id,
                    provider = %provider_name,
                    "Provider not available as metadata source, skipping"
                );
                continue;
            };

            info!(
                %artist_id,
                provider = %provider_name,
                %external_id,
                "Fetching bio from provider"
            );

            match provider.fetch_artist_bio(external_id).await {
                Some(bio) => {
                    let bio_len = bio.len();
                    if let Err(e) = db::update_artist_bio(&s.db, artist_id, Some(&bio)).await {
                        warn!(
                            %artist_id,
                            provider = %provider_name,
                            error = %e,
                            "Failed to persist fetched bio to database"
                        );
                        return;
                    }
                    {
                        let mut artists = s.monitored_artists.write().await;
                        if let Some(a) = artists.iter_mut().find(|a| a.id == artist_id) {
                            a.bio = Some(bio);
                        }
                    }
                    info!(
                        %artist_id,
                        provider = %provider_name,
                        bio_len,
                        "Successfully fetched and saved artist bio"
                    );
                    s.notify_sse();
                    return;
                }
                None => {
                    info!(
                        %artist_id,
                        provider = %provider_name,
                        %external_id,
                        "Provider returned no bio"
                    );
                }
            }
        }

        info!(
            %artist_id,
            "No provider returned a bio for this artist"
        );
    });
}

pub(crate) fn spawn_recompute_artist_match_suggestions(state: &AppState, artist_id: Uuid) {
    let s = state.clone();
    tokio::spawn(async move {
        if let Err(err) = services::recompute_artist_match_suggestions(&s, artist_id).await {
            warn!(artist_id = %artist_id, error = %err, "Background match recompute failed");
        }
        s.notify_sse();
    });
}

/// Find an existing artist by provider link, or create a new lightweight
/// (unmonitored) artist with a single provider link.
async fn find_or_create_lightweight_artist(
    state: &AppState,
    provider: &str,
    artist_external_id: &str,
    artist_name: &str,
) -> Result<Uuid, String> {
    // Check if artist already exists via provider link.
    if let Ok(Some(id)) =
        db::find_artist_by_provider_link(&state.db, provider, artist_external_id).await
    {
        return Ok(id);
    }

    let new_id = Uuid::now_v7();
    let external_url = default_provider_artist_url(provider, artist_external_id);
    let artist = yoink_shared::MonitoredArtist {
        id: new_id,
        name: artist_name.to_string(),
        image_url: None,
        bio: None,
        monitored: false, // lightweight — no auto-sync
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
        provider: provider.to_string(),
        external_id: artist_external_id.to_string(),
        external_url,
        external_name: Some(artist_name.to_string()),
        image_ref: None,
    };
    let _ = db::upsert_artist_provider_link(&state.db, &link).await;

    Ok(new_id)
}

/// Fetch tracks for an album from a provider and store them in the DB.
/// If `monitor_all` is true, every track is marked as monitored.
async fn store_album_tracks(
    state: &AppState,
    provider: &str,
    external_album_id: &str,
    album_id: Uuid,
    monitor_all: bool,
) -> Result<(), String> {
    let prov = state
        .registry
        .metadata_provider(provider)
        .ok_or_else(|| format!("Unknown metadata provider: {provider}"))?;

    let (tracks, _album_extra) = prov
        .fetch_tracks(external_album_id)
        .await
        .map_err(|e| format!("Failed to fetch tracks: {}", e.0))?;

    for track in tracks {
        let ext_id = track.external_id.clone();

        // Skip if this exact provider+external_id track already exists.
        let existing = db::find_track_by_provider_link(&state.db, provider, &ext_id)
            .await
            .ok()
            .flatten();
        if existing.is_some() {
            continue;
        }

        let secs = track.duration_secs;
        let mins = secs / 60;
        let rem = secs % 60;

        let track_info = yoink_shared::TrackInfo {
            id: Uuid::now_v7(),
            title: track.title,
            version: track.version,
            disc_number: track.disc_number.unwrap_or(1),
            track_number: track.track_number.max(1),
            duration_secs: secs,
            duration_display: format!("{mins}:{rem:02}"),
            isrc: track.isrc,
            explicit: track.explicit,
            track_artist: track.artists,
            file_path: None,
            monitored: monitor_all,
            acquired: false,
        };

        let _ = db::upsert_track(&state.db, &track_info, album_id).await;
        let _ = db::upsert_track_provider_link(&state.db, track_info.id, provider, &ext_id).await;
    }

    Ok(())
}

/// Execute a `ServerAction` against the real `AppState`.
pub(crate) async fn dispatch_action_impl(
    state: AppState,
    action: yoink_shared::ServerAction,
) -> Result<(), String> {
    use yoink_shared::ServerAction;

    match action {
        ServerAction::ToggleAlbumMonitor {
            album_id,
            monitored,
        } => {
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
                services::enqueue_album_download(&state, &album).await;
            }
            state.notify_sse();
        }

        ServerAction::BulkMonitor {
            artist_id,
            monitored,
        } => {
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
                services::enqueue_album_download(&state, &album).await;
            }
            state.notify_sse();
        }

        ServerAction::SyncArtistAlbums { artist_id } => {
            let _ = services::sync_artist_albums(&state, artist_id).await;
            {
                let artists = state.monitored_artists.read().await;
                let has_bio = artists
                    .iter()
                    .find(|a| a.id == artist_id)
                    .map(|a| a.bio.is_some())
                    .unwrap_or(false);
                if !has_bio {
                    spawn_fetch_artist_bio(&state, artist_id);
                }
            }
            spawn_recompute_artist_match_suggestions(&state, artist_id);
            state.notify_sse();
        }

        ServerAction::RemoveArtist {
            artist_id,
            remove_files,
        } => {
            if remove_files {
                let acquired: Vec<_> = {
                    let albums = state.monitored_albums.read().await;
                    albums
                        .iter()
                        .filter(|a| {
                            (a.artist_id == artist_id || a.artist_ids.contains(&artist_id))
                                && a.acquired
                        })
                        .cloned()
                        .collect()
                };
                for album in &acquired {
                    if let Err(e) = services::remove_downloaded_album_files(&state, album).await {
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
        }

        ServerAction::AddArtist {
            name,
            provider,
            external_id,
            image_url,
            external_url,
        } => {
            // Generate a default external URL if none was provided
            let external_url =
                external_url.or_else(|| default_provider_artist_url(&provider, &external_id));

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

            let _ = services::sync_artist_albums(&state, artist_id).await;
            spawn_fetch_artist_bio(&state, artist_id);
            spawn_recompute_artist_match_suggestions(&state, artist_id);
            state.notify_sse();
        }

        ServerAction::LinkArtistProvider {
            artist_id,
            provider,
            external_id,
            external_url,
            external_name,
            image_ref,
        } => {
            let external_url =
                external_url.or_else(|| default_provider_artist_url(&provider, &external_id));
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
                    spawn_fetch_artist_bio(&state, link.artist_id);
                }
            }
            spawn_recompute_artist_match_suggestions(&state, link.artist_id);
            state.notify_sse();
        }

        ServerAction::UnlinkArtistProvider {
            artist_id,
            provider,
            external_id,
        } => {
            let _ = db::delete_artist_provider_link(&state.db, artist_id, &provider, &external_id)
                .await;
            spawn_recompute_artist_match_suggestions(&state, artist_id);
            state.notify_sse();
        }

        ServerAction::AcceptMatchSuggestion { suggestion_id } => {
            let suggestion = db::load_match_suggestion_by_id(&state.db, suggestion_id)
                .await
                .map_err(|e| format!("failed to load match suggestion: {e}"))?
                .ok_or_else(|| "match suggestion not found".to_string())?;

            match suggestion.scope_type.as_str() {
                "album" => {
                    let album_links = db::load_album_provider_links(&state.db, suggestion.scope_id)
                        .await
                        .map_err(|e| format!("failed loading album links: {e}"))?;
                    let linked: std::collections::HashSet<(String, String)> = album_links
                        .iter()
                        .map(|l| (l.provider.clone(), l.external_id.clone()))
                        .collect();
                    let left_linked = linked.contains(&(
                        suggestion.left_provider.clone(),
                        suggestion.left_external_id.clone(),
                    ));
                    let right_linked = linked.contains(&(
                        suggestion.right_provider.clone(),
                        suggestion.right_external_id.clone(),
                    ));
                    let (target_provider, target_external_id, target_url) =
                        if left_linked && !right_linked {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        } else if right_linked && !left_linked {
                            (
                                suggestion.left_provider.clone(),
                                suggestion.left_external_id.clone(),
                                None,
                            )
                        } else {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        };

                    let existing = db::find_album_by_provider_link(
                        &state.db,
                        &target_provider,
                        &target_external_id,
                    )
                    .await
                    .map_err(|e| format!("failed checking existing album link: {e}"))?;

                    if let Some(existing_album_id) = existing
                        && existing_album_id != suggestion.scope_id
                    {
                        return Err(
                            "Cannot accept: provider album is already linked to another local album"
                                .to_string(),
                        );
                    }

                    let link = db::AlbumProviderLink {
                        id: Uuid::now_v7(),
                        album_id: suggestion.scope_id,
                        provider: target_provider,
                        external_id: target_external_id,
                        external_url: target_url,
                        external_title: suggestion.external_name.clone(),
                        cover_ref: None,
                    };
                    let _ = db::upsert_album_provider_link(&state.db, &link).await;
                }
                "artist" => {
                    let artist_links =
                        db::load_artist_provider_links(&state.db, suggestion.scope_id)
                            .await
                            .map_err(|e| format!("failed loading artist links: {e}"))?;
                    let linked: std::collections::HashSet<(String, String)> = artist_links
                        .iter()
                        .map(|l| (l.provider.clone(), l.external_id.clone()))
                        .collect();
                    let left_linked = linked.contains(&(
                        suggestion.left_provider.clone(),
                        suggestion.left_external_id.clone(),
                    ));
                    let right_linked = linked.contains(&(
                        suggestion.right_provider.clone(),
                        suggestion.right_external_id.clone(),
                    ));
                    let (target_provider, target_external_id, target_url) =
                        if left_linked && !right_linked {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        } else if right_linked && !left_linked {
                            (
                                suggestion.left_provider.clone(),
                                suggestion.left_external_id.clone(),
                                None,
                            )
                        } else {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        };

                    let existing = db::find_artist_by_provider_link(
                        &state.db,
                        &target_provider,
                        &target_external_id,
                    )
                    .await
                    .map_err(|e| format!("failed checking existing artist link: {e}"))?;

                    if let Some(existing_artist_id) = existing
                        && existing_artist_id != suggestion.scope_id
                    {
                        return Err(
                            "Cannot accept: provider artist is already linked to another local artist"
                                .to_string(),
                        );
                    }

                    let link = db::ArtistProviderLink {
                        id: Uuid::now_v7(),
                        artist_id: suggestion.scope_id,
                        provider: target_provider,
                        external_id: target_external_id,
                        external_url: target_url,
                        external_name: suggestion.external_name.clone(),
                        image_ref: None,
                    };
                    let _ = db::upsert_artist_provider_link(&state.db, &link).await;

                    let _ = services::sync_artist_albums(&state, suggestion.scope_id).await;
                    spawn_recompute_artist_match_suggestions(&state, suggestion.scope_id);
                }
                _ => return Err("unknown suggestion scope type".to_string()),
            }

            let _ = db::set_match_suggestion_status(&state.db, suggestion_id, "accepted").await;

            if suggestion.scope_type == "album" {
                let artist_id = {
                    let albums = state.monitored_albums.read().await;
                    albums
                        .iter()
                        .find(|a| a.id == suggestion.scope_id)
                        .map(|a| a.artist_id)
                };
                if let Some(artist_id) = artist_id {
                    spawn_recompute_artist_match_suggestions(&state, artist_id);
                }
            }

            state.notify_sse();
        }

        ServerAction::DismissMatchSuggestion { suggestion_id } => {
            let scope = db::load_match_suggestion_by_id(&state.db, suggestion_id)
                .await
                .ok()
                .flatten();
            let _ = db::set_match_suggestion_status(&state.db, suggestion_id, "dismissed").await;

            if let Some(suggestion) = scope
                && suggestion.scope_type == "album"
            {
                let artist_id = {
                    let albums = state.monitored_albums.read().await;
                    albums
                        .iter()
                        .find(|a| a.id == suggestion.scope_id)
                        .map(|a| a.artist_id)
                };
                if let Some(artist_id) = artist_id {
                    spawn_recompute_artist_match_suggestions(&state, artist_id);
                }
            }
            state.notify_sse();
        }

        ServerAction::RefreshMatchSuggestions { artist_id } => {
            let _ = services::recompute_artist_match_suggestions(&state, artist_id).await;
            state.notify_sse();
        }

        ServerAction::MergeAlbums {
            target_album_id,
            source_album_id,
            result_title,
            result_cover_url,
        } => {
            services::merge_albums(
                &state,
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
                spawn_recompute_artist_match_suggestions(&state, artist_id);
            }
            state.notify_sse();
        }

        ServerAction::CancelDownload { job_id } => {
            let mut jobs = state.download_jobs.write().await;
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id)
                && matches!(job.status, yoink_shared::DownloadStatus::Queued)
            {
                job.status = yoink_shared::DownloadStatus::Failed;
                job.error = Some("Cancelled by user".to_string());
                job.updated_at = Utc::now();
                let _ = db::update_job(&state.db, job).await;
                info!(%job_id, "Cancelled download job");
            }
            drop(jobs);
            state.notify_sse();
        }

        ServerAction::ClearCompleted => {
            let _ = db::delete_completed_jobs(&state.db).await;
            {
                let mut jobs = state.download_jobs.write().await;
                jobs.retain(|j| j.status != yoink_shared::DownloadStatus::Completed);
            }
            info!("Cleared completed download jobs");
            state.notify_sse();
        }

        ServerAction::RetryDownload { album_id } => {
            {
                let mut jobs = state.download_jobs.write().await;
                if let Some(job) = jobs.iter_mut().find(|j| {
                    j.album_id == album_id && j.status == yoink_shared::DownloadStatus::Failed
                }) {
                    let previous_quality = job.quality;
                    job.status = yoink_shared::DownloadStatus::Queued;
                    job.quality = state.default_quality;
                    job.error = None;
                    job.updated_at = Utc::now();
                    let _ = db::update_job(&state.db, job).await;
                    info!(
                        %album_id,
                        job_id = %job.id,
                        previous_quality = %previous_quality,
                        retry_quality = %job.quality,
                        "Retrying failed download job"
                    );
                    state.download_notify.notify_one();
                    state.notify_sse();
                    return Ok(());
                }
            }
            let album = {
                let albums = state.monitored_albums.read().await;
                albums.iter().find(|a| a.id == album_id).cloned()
            };
            if let Some(album) = album {
                info!(album_id = %album.id, title = %album.title, "Creating retry download job");
                services::enqueue_album_download(&state, &album).await;
            }
            state.notify_sse();
        }

        ServerAction::RemoveAlbumFiles {
            album_id,
            unmonitor,
        } => {
            let album = {
                let albums = state.monitored_albums.read().await;
                albums.iter().find(|a| a.id == album_id).cloned()
            }
            .ok_or_else(|| format!("album {} not found", album_id))?;

            let removed = services::remove_downloaded_album_files(&state, &album).await?;

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
                    let should_remove = j.album_id == album_id
                        && j.status == yoink_shared::DownloadStatus::Completed;
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
                services::enqueue_album_download(&state, &album).await;
            }

            info!(
                %album_id,
                removed, unmonitor, "Removed downloaded album files"
            );
            state.notify_sse();
        }

        ServerAction::AddAlbumArtist {
            album_id,
            artist_id,
        } => {
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
        }

        ServerAction::RemoveAlbumArtist {
            album_id,
            artist_id,
        } => {
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
        }

        ServerAction::RetagLibrary => {
            let s = state.clone();
            tokio::spawn(async move {
                match services::retag_existing_files(&s).await {
                    Ok((tagged, missing, albums)) => {
                        info!(
                            tagged_files = tagged,
                            missing_files = missing,
                            scanned_albums = albums,
                            "Completed manual library retag"
                        );
                    }
                    Err(err) => {
                        info!(error = %err, "Library retag failed");
                    }
                }
            });
        }

        ServerAction::ScanImportLibrary => {
            let s = state.clone();
            tokio::spawn(async move {
                match services::scan_and_import_library(&s).await {
                    Ok(summary) => {
                        info!(
                            discovered = summary.discovered_albums,
                            imported = summary.imported_albums,
                            artists_added = summary.artists_added,
                            unmatched = summary.unmatched_albums,
                            "Completed scan/import pass"
                        );
                    }
                    Err(err) => {
                        info!(error = %err, "Scan/import failed");
                    }
                }
            });
        }

        ServerAction::UpdateArtist {
            artist_id,
            name,
            image_url,
        } => {
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
        }

        ServerAction::FetchArtistBio { artist_id } => {
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
            spawn_fetch_artist_bio(&state, artist_id);
        }

        ServerAction::ConfirmImport { items } => {
            let summary = services::confirm_import_library(&state, items).await?;
            info!(
                total = summary.total_selected,
                imported = summary.imported,
                artists_added = summary.artists_added,
                failed = summary.failed,
                "Confirmed import completed"
            );
            if !summary.errors.is_empty() {
                return Err(format!(
                    "Imported {}/{} albums. Errors: {}",
                    summary.imported,
                    summary.total_selected,
                    summary.errors.join("; ")
                ));
            }
        }

        ServerAction::ToggleArtistMonitor {
            artist_id,
            monitored,
        } => {
            let _ = db::update_artist_monitored(&state.db, artist_id, monitored).await;
            {
                let mut artists = state.monitored_artists.write().await;
                if let Some(artist) = artists.iter_mut().find(|a| a.id == artist_id) {
                    artist.monitored = monitored;
                }
            }
            if monitored {
                // Promoting to fully monitored — sync discography from providers
                let _ = services::sync_artist_albums(&state, artist_id).await;
                spawn_fetch_artist_bio(&state, artist_id);
                spawn_recompute_artist_match_suggestions(&state, artist_id);
            }
            info!(%artist_id, monitored, "Toggled artist monitored status");
            state.notify_sse();
        }

        ServerAction::AddAlbum {
            provider,
            external_album_id,
            artist_external_id,
            artist_name,
            monitor_all,
        } => {
            // 1. Find or create lightweight (unmonitored) artist.
            let artist_id = find_or_create_lightweight_artist(
                &state,
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
            store_album_tracks(&state, &provider, &external_album_id, album_id, monitor_all)
                .await?;

            // 5. If monitored, queue download.
            if monitor_all {
                let album = {
                    let albums = state.monitored_albums.read().await;
                    albums.iter().find(|a| a.id == album_id).cloned()
                };
                if let Some(album) = album {
                    services::enqueue_album_download(&state, &album).await;
                }
            }

            info!(%album_id, %provider, %external_album_id, monitor_all, "Added album from search");
            state.notify_sse();
        }

        ServerAction::AddTrack {
            provider,
            external_track_id,
            external_album_id,
            artist_external_id,
            artist_name,
        } => {
            // 1. Find or create lightweight (unmonitored) artist.
            let artist_id = find_or_create_lightweight_artist(
                &state,
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
            store_album_tracks(&state, &provider, &external_album_id, album_id, false).await?;

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
                            services::enqueue_album_download(&state, &album_clone).await;
                        }
                    }
                }
            }

            info!(%album_id, %provider, %external_track_id, "Added track from search");
            state.notify_sse();
        }

        ServerAction::ToggleTrackMonitor {
            track_id,
            album_id,
            monitored,
        } => {
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
                        services::enqueue_album_download(&state, &album_clone).await;
                    }
                }
            }
            info!(%track_id, %album_id, monitored, "Toggled track monitored status");
            state.notify_sse();
        }

        ServerAction::BulkToggleTrackMonitor {
            album_id,
            monitored,
        } => {
            // Update all tracks for the album
            let tracks = db::load_tracks_for_album(&state.db, album_id)
                .await
                .unwrap_or_default();

            for track in &tracks {
                let _ =
                    db::update_track_flags(&state.db, track.id, monitored, track.acquired).await;
            }

            // Recompute the album's partially_wanted flag and potentially enqueue
            {
                let mut albums = state.monitored_albums.write().await;
                if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
                    services::recompute_partially_wanted(&state.db, album).await;
                    if album.wanted || album.partially_wanted {
                        let album_clone = album.clone();
                        drop(albums);
                        services::enqueue_album_download(&state, &album_clone).await;
                    }
                }
            }

            let count = tracks.len();
            info!(%album_id, monitored, count, "Bulk toggled track monitoring");
            state.notify_sse();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::db;
    use crate::models::DownloadStatus;
    use crate::providers::registry::ProviderRegistry;
    use crate::providers::{ProviderAlbum, ProviderAlbumArtist, ProviderTrack};
    use crate::test_helpers::*;
    use yoink_shared::ServerAction;

    use super::dispatch_action_impl;

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
            super::default_provider_artist_url("tidal", "123"),
            Some("https://tidal.com/browse/artist/123".to_string())
        );
        assert_eq!(
            super::default_provider_artist_url("deezer", "456"),
            Some("https://www.deezer.com/artist/456".to_string())
        );
        assert_eq!(
            super::default_provider_artist_url("musicbrainz", "abc"),
            Some("https://musicbrainz.org/artist/abc".to_string())
        );
        assert!(super::default_provider_artist_url("unknown", "x").is_none());
    }
}
