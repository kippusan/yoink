use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{db, services, state::AppState};

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
                for album in albums.iter_mut().filter(|a| {
                    a.artist_id == artist_id || a.artist_ids.contains(&artist_id)
                }) {
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
                    let is_related = album.artist_id == artist_id
                        || album.artist_ids.contains(&artist_id);
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
            let _ =
                db::delete_artist_provider_link(&state.db, artist_id, &provider, &external_id)
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
                    let album_links =
                        db::load_album_provider_links(&state.db, suggestion.scope_id)
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
                    let previous_quality = job.quality.clone();
                    job.status = yoink_shared::DownloadStatus::Queued;
                    job.quality = state.default_quality.clone();
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
                if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
                    if !album.artist_ids.contains(&artist_id) {
                        album.artist_ids.push(artist_id);
                    }
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
                if let Some(album) = albums.iter().find(|a| a.id == album_id) {
                    if album.artist_ids.len() <= 1 {
                        return Err("Cannot remove the only artist from an album".to_string());
                    }
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
    }

    Ok(())
}
