mod io;
mod lyrics;
mod manifest;
mod metadata;
mod worker;

pub(crate) use io::sanitize_path_component;
pub(crate) use metadata::write_audio_metadata;

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use tokio::fs;
use tracing::{debug, error, info};

use crate::{
    db,
    models::{
        DownloadJob, DownloadStatus, HifiAlbumItem, HifiAlbumResponse, MonitoredAlbum,
    },
    state::AppState,
};

use io::{normalize_quality, parse_track_number_from_path, sanitize_path_component as sanitize};
use metadata::{
    build_full_artist_string, extract_disc_number, fetch_cover_art_bytes, fetch_track_info_extra,
};
use super::hifi::hifi_get_json;
use super::library::update_wanted;
use worker::download_album_job;

pub(crate) async fn enqueue_album_download(state: &AppState, album: &MonitoredAlbum) {
    if !album.monitored || album.acquired {
        debug!(
            album_id = album.id,
            monitored = album.monitored,
            acquired = album.acquired,
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
            album_id = album.id,
            "Skipping enqueue because active job exists"
        );
        return;
    }

    let requested_quality = normalize_quality(&state.default_quality);

    let mut new_job = DownloadJob {
        id: 0, // will be set from DB
        album_id: album.id,
        artist_id: album.artist_id,
        album_title: album.title.clone(),
        status: DownloadStatus::Queued,
        quality: requested_quality.clone(),
        total_tracks: 0,
        completed_tracks: 0,
        error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Persist to DB and get the auto-increment ID
    match db::insert_job(&state.db, &new_job).await {
        Ok(row_id) => new_job.id = row_id as u64,
        Err(err) => {
            error!(error = %err, "Failed to persist download job to database");
            let next_id = jobs.iter().map(|job| job.id).max().unwrap_or(0) + 1;
            new_job.id = next_id;
        }
    }

    info!(
        job_id = new_job.id,
        album_id = album.id,
        artist_id = album.artist_id,
        title = %album.title,
        quality = %requested_quality,
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
            job_id = job.id,
            album_id = job.album_id,
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
                    job_id = job.id,
                    album_id = job.album_id,
                    "Download job completed"
                );
                let mut albums = state.monitored_albums.write().await;
                if let Some(album) = albums.iter_mut().find(|album| album.id == job.album_id) {
                    album.acquired = true;
                    update_wanted(album);
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
                error!(job_id = job.id, album_id = job.album_id, error = %err, "Download job failed");
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

    let artist_names: HashMap<i64, String> = artists.into_iter().map(|a| (a.id, a.name)).collect();

    let mut tagged_files = 0usize;
    let mut missing_files = 0usize;
    let mut scanned_albums = 0usize;

    for album in albums.into_iter().filter(|a| a.acquired) {
        let Some(artist_name) = artist_names.get(&album.artist_id) else {
            continue;
        };

        let release_suffix = album
            .release_date
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        let cover_art = fetch_cover_art_bytes(&state.http, album.cover.as_deref()).await;
        let album_dir = state
            .music_root
            .join(sanitize(artist_name))
            .join(sanitize(&format!(
                "{} ({})",
                album.title, release_suffix
            )));

        if !fs::try_exists(&album_dir).await.unwrap_or(false) {
            continue;
        }

        scanned_albums += 1;

        let response = hifi_get_json::<HifiAlbumResponse>(
            state,
            "/album/",
            vec![("id".to_string(), album.id.to_string())],
        )
        .await?;

        let album_extra = response.data.extra;
        let tracks = response
            .data
            .items
            .into_iter()
            .enumerate()
            .map(|(idx, item)| match item {
                HifiAlbumItem::Item { item } => (idx, item),
                HifiAlbumItem::Track(item) => (idx, item),
            })
            .collect::<Vec<_>>();
        let total_tracks = tracks.len() as u32;

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

        for (idx, track) in tracks {
            let track_number = track.track_number.unwrap_or((idx + 1) as u32);
            let track_info_extra = fetch_track_info_extra(state, track.id).await;
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

            write_audio_metadata(
                &path,
                &track.title,
                &track_artist,
                artist_name,
                &album.title,
                track_number,
                disc_number,
                total_tracks,
                &release_suffix,
                &track.extra,
                &album_extra,
                track_info_extra.as_ref(),
                None,
                cover_art.as_deref(),
            )?;
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
    let album_dir = artist_dir.join(sanitize(&format!(
        "{} ({})",
        album.title, release_suffix
    )));

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
