mod io;
mod lyrics;
mod metadata;
mod worker;

pub(crate) use io::sanitize_path_component;
pub(crate) use metadata::{TrackMetadata, write_audio_metadata};

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use tokio::fs;
use tracing::{debug, error, info};

use uuid::Uuid;

use crate::{
    db,
    models::{DownloadJob, DownloadStatus, MonitoredAlbum},
    state::AppState,
};

use super::library::{recompute_partially_wanted, update_wanted};
use io::{parse_track_number_from_path, sanitize_path_component as sanitize};
use metadata::{build_full_artist_string, extract_disc_number};
use worker::download_album_job;

pub(crate) async fn enqueue_album_download(state: &AppState, album: &MonitoredAlbum) {
    // Enqueue if the album is fully wanted (monitored && !acquired)
    // OR partially wanted (has individually monitored tracks not yet acquired).
    let dominated = album.monitored && !album.acquired;
    let partial = album.partially_wanted;
    if !dominated && !partial {
        debug!(
            album_id = %album.id,
            monitored = album.monitored,
            acquired = album.acquired,
            partially_wanted = album.partially_wanted,
            "Skipping enqueue because album is not wanted"
        );
        return;
    }

    let mut jobs = state.download_jobs.write().await;
    if jobs.iter().any(|job| {
        job.album_id == album.id
            && matches!(
                job.status,
                DownloadStatus::Queued | DownloadStatus::Resolving | DownloadStatus::Downloading
            )
    }) {
        debug!(
            album_id = %album.id,
            "Skipping enqueue because active job exists"
        );
        return;
    }

    let requested_quality = state.default_quality.clone();

    // Resolve artist name for denormalization
    let artist_name = {
        let artists = state.monitored_artists.read().await;
        artists
            .iter()
            .find(|a| a.id == album.artist_id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Unknown Artist".to_string())
    };

    // Determine the download source from available album provider links
    let source = {
        let links = db::load_album_provider_links(&state.db, album.id)
            .await
            .unwrap_or_default();
        let download_sources = state.registry.download_sources();
        let download_source_ids: Vec<String> = download_sources
            .iter()
            .map(|s| s.id().to_string())
            .collect();

        // Prefer a linked provider that is also a download source.
        let linked = links
            .iter()
            .find(|l| download_source_ids.contains(&l.provider))
            .map(|l| l.provider.clone());

        if let Some(id) = linked {
            id
        } else {
            // Fall back to a source that can operate without provider-linked IDs.
            download_sources
                .iter()
                .find(|s| !s.requires_linked_provider())
                .map(|s| s.id().to_string())
                .or_else(|| download_source_ids.first().cloned())
                .unwrap_or_else(|| "tidal".to_string())
        }
    };

    let mut new_job = DownloadJob {
        id: Uuid::now_v7(),
        album_id: album.id,
        source,
        album_title: album.title.clone(),
        artist_name,
        status: DownloadStatus::Queued,
        quality: requested_quality.as_str().to_string(),
        total_tracks: 0,
        completed_tracks: 0,
        error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Persist to DB
    match db::insert_job(&state.db, &new_job).await {
        Ok(persisted_id) => new_job.id = persisted_id,
        Err(err) => {
            error!(error = %err, "Failed to persist download job to database");
        }
    }

    info!(
        job_id = %new_job.id,
        album_id = %album.id,
        title = %album.title,
        quality = %requested_quality,
        source = %new_job.source,
        "Queued album download"
    );
    jobs.push(new_job);
    drop(jobs);
    state.download_notify.notify_one();
}

pub(crate) async fn download_worker_loop(state: AppState) {
    info!("Download worker started");
    loop {
        let next_job = {
            let mut jobs = state.download_jobs.write().await;
            if let Some(job) = jobs
                .iter_mut()
                .find(|job| job.status == DownloadStatus::Queued)
            {
                job.status = DownloadStatus::Resolving;
                job.updated_at = Utc::now();
                let _ = db::update_job(&state.db, job).await;
                Some(job.clone())
            } else {
                None
            }
        };

        let Some(job) = next_job else {
            state.download_notify.notified().await;
            continue;
        };

        info!(
            job_id = %job.id,
            album_id = %job.album_id,
            "Processing download job"
        );

        let outcome = download_album_job(&state, job.clone()).await;
        match outcome {
            Ok(()) => {
                {
                    let mut jobs = state.download_jobs.write().await;
                    if let Some(existing) = jobs.iter_mut().find(|item| item.id == job.id) {
                        existing.status = DownloadStatus::Completed;
                        existing.error = None;
                        existing.updated_at = Utc::now();
                        let _ = db::update_job(&state.db, existing).await;
                    }
                }
                info!(
                    job_id = %job.id,
                    album_id = %job.album_id,
                    "Download job completed"
                );
                let mut albums = state.monitored_albums.write().await;
                if let Some(album) = albums.iter_mut().find(|album| album.id == job.album_id) {
                    if album.monitored {
                        // Fully monitored album: all tracks were downloaded
                        album.acquired = true;
                    } else {
                        // Partially wanted album: acquired only if every
                        // monitored track is now acquired.
                        album.acquired = db::all_monitored_tracks_acquired(
                            &state.db,
                            album.id,
                        )
                        .await
                        .unwrap_or(false);
                    }
                    update_wanted(album);
                    recompute_partially_wanted(&state.db, album).await;
                    let _ = db::update_album_flags(
                        &state.db,
                        album.id,
                        album.monitored,
                        album.acquired,
                        album.wanted,
                    )
                    .await;
                }
                state.notify_sse();
            }
            Err(err) => {
                error!(job_id = %job.id, album_id = %job.album_id, error = %err, "Download job failed");
                {
                    let mut jobs = state.download_jobs.write().await;
                    if let Some(existing) = jobs.iter_mut().find(|item| item.id == job.id) {
                        existing.status = DownloadStatus::Failed;
                        existing.error = Some(err.clone());
                        existing.updated_at = Utc::now();
                        let _ = db::update_job(&state.db, existing).await;
                    }
                }
                let mut albums = state.monitored_albums.write().await;
                if let Some(album) = albums.iter_mut().find(|album| album.id == job.album_id) {
                    album.acquired = false;
                    update_wanted(album);
                    recompute_partially_wanted(&state.db, album).await;
                    let _ = db::update_album_flags(
                        &state.db,
                        album.id,
                        album.monitored,
                        album.acquired,
                        album.wanted,
                    )
                    .await;
                }
                state.notify_sse();
            }
        }
    }
}

pub(crate) async fn retag_existing_files(
    state: &AppState,
) -> Result<(usize, usize, usize), String> {
    let artists = state.monitored_artists.read().await.clone();
    let albums = state.monitored_albums.read().await.clone();

    let artist_names: HashMap<Uuid, String> = artists.into_iter().map(|a| (a.id, a.name)).collect();

    let mut tagged_files = 0usize;
    let mut missing_files = 0usize;
    let mut scanned_albums = 0usize;

    for album in albums.into_iter().filter(|a| a.acquired) {
        let Some(artist_name) = artist_names.get(&album.artist_id) else {
            continue;
        };

        // Find a metadata provider link for this album
        let album_links = db::load_album_provider_links(&state.db, album.id)
            .await
            .unwrap_or_default();

        // Find the first link that has a matching metadata provider
        let provider_link = album_links
            .iter()
            .find(|l| state.registry.metadata_provider(&l.provider).is_some());
        let Some(link) = provider_link else {
            continue;
        };

        let provider = state.registry.metadata_provider(&link.provider).unwrap();

        let release_suffix = album
            .release_date
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        // Fetch cover art via provider
        let cover_art = if let Some(cover_ref) = link.cover_ref.as_deref() {
            provider.fetch_cover_art_bytes(cover_ref).await
        } else {
            fetch_cover_art_bytes_from_url(&state.http, album.cover_url.as_deref()).await
        };

        let album_dir = state
            .music_root
            .join(sanitize(artist_name))
            .join(sanitize(&format!("{} ({})", album.title, release_suffix)));

        if !fs::try_exists(&album_dir).await.unwrap_or(false) {
            continue;
        }

        scanned_albums += 1;

        let (provider_tracks, album_extra) = match provider.fetch_tracks(&link.external_id).await {
            Ok(result) => result,
            Err(err) => {
                info!(
                    album_id = %album.id,
                    provider = %link.provider,
                    error = %err.0,
                    "Failed to fetch tracks for retagging"
                );
                continue;
            }
        };

        let total_tracks = provider_tracks.len() as u32;

        let mut files_by_track: HashMap<u32, PathBuf> = HashMap::new();
        let mut ordered_files: Vec<PathBuf> = Vec::new();

        let mut entries = fs::read_dir(&album_dir).await.map_err(|err| {
            format!(
                "failed to read album directory {}: {err}",
                album_dir.display()
            )
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|err| {
            format!(
                "failed to read directory entry {}: {err}",
                album_dir.display()
            )
        })? {
            let path = entry.path();
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !(ext.eq_ignore_ascii_case("flac")
                || ext.eq_ignore_ascii_case("m4a")
                || ext.eq_ignore_ascii_case("mp4"))
            {
                continue;
            }

            if let Some(track_number) = parse_track_number_from_path(&path) {
                files_by_track.insert(track_number, path.clone());
            }
            ordered_files.push(path);
        }

        ordered_files.sort();

        for (idx, track) in provider_tracks.iter().enumerate() {
            let track_number = track.track_number;
            let track_info_extra = provider.fetch_track_info_extra(&track.external_id).await;
            let track_artist = build_full_artist_string(
                &track.title,
                &track.extra,
                track_info_extra.as_ref(),
                artist_name,
            );
            let disc_number = extract_disc_number(&track.extra, track_info_extra.as_ref());
            let path = files_by_track
                .get(&track_number)
                .cloned()
                .or_else(|| ordered_files.get(idx).cloned());

            let Some(path) = path else {
                missing_files += 1;
                continue;
            };

            write_audio_metadata(&TrackMetadata {
                path: &path,
                title: &track.title,
                track_artist: &track_artist,
                album_artist: artist_name,
                album: &album.title,
                track_number,
                disc_number,
                total_tracks,
                release_date: &release_suffix,
                track_extra: &track.extra,
                album_extra: &album_extra,
                track_info_extra: track_info_extra.as_ref(),
                lyrics_text: None,
                cover_art_jpeg: cover_art.as_deref(),
            })?;
            tagged_files += 1;
        }
    }

    Ok((tagged_files, missing_files, scanned_albums))
}

pub(crate) async fn remove_downloaded_album_files(
    state: &AppState,
    album: &MonitoredAlbum,
) -> Result<bool, String> {
    let artist_name = {
        let artists = state.monitored_artists.read().await;
        artists
            .iter()
            .find(|artist| artist.id == album.artist_id)
            .map(|artist| artist.name.clone())
            .unwrap_or_else(|| "Unknown Artist".to_string())
    };

    let release_suffix = album
        .release_date
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());

    let artist_dir = state.music_root.join(sanitize(&artist_name));
    let album_dir = artist_dir.join(sanitize(&format!("{} ({})", album.title, release_suffix)));

    let exists = fs::try_exists(&album_dir).await.map_err(|err| {
        format!(
            "failed to check album directory {}: {err}",
            album_dir.display()
        )
    })?;
    if !exists {
        return Ok(false);
    }

    fs::remove_dir_all(&album_dir).await.map_err(|err| {
        format!(
            "failed to remove album directory {}: {err}",
            album_dir.display()
        )
    })?;

    if let Ok(mut entries) = fs::read_dir(&artist_dir).await {
        let is_empty = entries.next_entry().await.ok().flatten().is_none();
        if is_empty {
            let _ = fs::remove_dir(&artist_dir).await;
        }
    }

    Ok(true)
}

/// Fetch cover art from an already-resolved URL (or a provider image proxy URL).
async fn fetch_cover_art_bytes_from_url(
    http: &reqwest::Client,
    cover_url: Option<&str>,
) -> Option<Vec<u8>> {
    let url = cover_url?;
    if url.starts_with('/') {
        // It's a proxy URL like /api/image/tidal/xxx/640
        // Extract the image_ref and call the Tidal URL directly
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() >= 5 && parts[3] == "tidal" {
            let image_ref = parts[4];
            let size = parts.get(5).unwrap_or(&"640");
            let tidal_url = format!(
                "https://resources.tidal.com/images/{}/{}x{}.jpg",
                image_ref.replace('-', "/"),
                size,
                size
            );
            let resp = http.get(&tidal_url).send().await.ok()?;
            if resp.status().is_success() {
                return resp.bytes().await.ok().map(|b| b.to_vec());
            }
        }
        return None;
    }
    let resp = http.get(url).send().await.ok()?;
    if resp.status().is_success() {
        resp.bytes().await.ok().map(|b| b.to_vec())
    } else {
        None
    }
}
