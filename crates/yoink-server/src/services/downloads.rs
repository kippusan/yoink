use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::Engine;
use chrono::Utc;
use lofty::{
    config::WriteOptions,
    file::{AudioFile, TaggedFileExt},
    picture::{MimeType, Picture, PictureType},
    prelude::{Accessor, ItemKey},
    probe::Probe,
    tag::{Tag, TagType},
};
use lrclib_api_rs::{LRCLibAPI, types::GetLyricsResponse};
use roxmltree::{Document, Node};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};
use tracing::{debug, error, info, warn};

use crate::{
    config::DOWNLOAD_CHUNK_SIZE,
    db,
    models::{
        BtsManifest, DownloadJob, DownloadStatus, HifiAlbumItem, HifiAlbumResponse,
        HifiPlaybackData, HifiPlaybackResponse, MonitoredAlbum,
    },
    state::AppState,
};
use serde_json::Value;

use super::{hifi::hifi_get_json, library::update_wanted};

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
            .join(sanitize_path_component(artist_name))
            .join(sanitize_path_component(&format!(
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

    let artist_dir = state.music_root.join(sanitize_path_component(&artist_name));
    let album_dir = artist_dir.join(sanitize_path_component(&format!(
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

async fn download_album_job(state: &AppState, job: DownloadJob) -> Result<(), String> {
    let requested_quality = normalize_quality(&job.quality);

    info!(
        job_id = job.id,
        album_id = job.album_id,
        artist_id = job.artist_id,
        quality = %requested_quality,
        "Starting album download"
    );
    let artist_name = {
        let artists = state.monitored_artists.read().await;
        artists
            .iter()
            .find(|artist| artist.id == job.artist_id)
            .map(|artist| artist.name.clone())
            .unwrap_or_else(|| "Unknown Artist".to_string())
    };

    let album_response = hifi_get_json::<HifiAlbumResponse>(
        state,
        "/album/",
        vec![("id".to_string(), job.album_id.to_string())],
    )
    .await?;

    let tracks = album_response
        .data
        .items
        .into_iter()
        .enumerate()
        .map(|(idx, item)| match item {
            HifiAlbumItem::Item { item } => (idx, item),
            HifiAlbumItem::Track(item) => (idx, item),
        })
        .collect::<Vec<_>>();
    let album_extra = album_response.data.extra;

    if tracks.is_empty() {
        return Err("Album has no downloadable tracks".to_string());
    }

    let total_tracks = tracks.len();
    update_job_progress(
        state,
        job.id,
        total_tracks,
        0,
        DownloadStatus::Downloading,
        None,
    )
    .await;

    let album = {
        let albums = state.monitored_albums.read().await;
        albums
            .iter()
            .find(|album| album.id == job.album_id)
            .cloned()
    };
    let release_suffix = album
        .as_ref()
        .and_then(|album| album.release_date.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let cover_art =
        fetch_cover_art_bytes(&state.http, album.as_ref().and_then(|a| a.cover.as_deref())).await;

    let artist_dir = state.music_root.join(sanitize_path_component(&artist_name));
    let album_dir = artist_dir.join(sanitize_path_component(&format!(
        "{} ({})",
        job.album_title, release_suffix
    )));
    fs::create_dir_all(&album_dir)
        .await
        .map_err(|err| format!("failed to create output directory: {err}"))?;

    for (idx, track) in tracks {
        debug!(
            job_id = job.id,
            album_id = job.album_id,
            track_id = track.id,
            track_number = track.track_number,
            track_title = %track.title,
            "Resolving track playback"
        );
        let playback = hifi_get_json::<HifiPlaybackResponse>(
            state,
            "/track/",
            vec![
                ("id".to_string(), track.id.to_string()),
                ("quality".to_string(), requested_quality.clone()),
            ],
        )
        .await?;
        let track_payload = match extract_download_payload(&playback.data) {
            Ok(payload) => payload,
            Err(err)
                if playback.data.manifest_mime_type == "application/dash+xml"
                    && requested_quality == "HI_RES_LOSSLESS" =>
            {
                let dash_summary = summarize_manifest_for_logs(&playback.data);
                warn!(
                    track_id = track.id,
                    album_id = job.album_id,
                    error = %err,
                    manifest_summary = %dash_summary,
                    "HI_RES DASH manifest unsupported, falling back to LOSSLESS"
                );

                fetch_track_payload_for_quality(state, track.id, "LOSSLESS").await?
            }
            Err(err) => return Err(err),
        };

        if matches!(track_payload, DownloadPayload::DashSegmentUrls(_)) {
            info!(
                track_id = track.id,
                album_id = job.album_id,
                manifest_summary = %summarize_manifest_for_logs(&playback.data),
                "Using DASH segment download for track"
            );
        }
        let track_number = track.track_number.unwrap_or((idx + 1) as u32);
        let track_info_extra = fetch_track_info_extra(state, track.id).await;
        let track_artist = build_full_artist_string(
            &track.title,
            &track.extra,
            track_info_extra.as_ref(),
            &artist_name,
        );
        let disc_number = extract_disc_number(&track.extra, track_info_extra.as_ref());
        let lyrics = if state.download_lyrics {
            fetch_track_lyrics(
                state,
                &track.title,
                &artist_name,
                &job.album_title,
                track.duration,
            )
            .await
        } else {
            None
        };
        let base_name = format!(
            "{:02} - {}",
            track_number,
            sanitize_path_component(&track.title)
        );
        let temp_path = album_dir.join(format!("{base_name}.part"));

        download_payload_to_file(&state.http, &track_payload, &temp_path)
            .await
            .map_err(|err| format!("failed track {}: {err}", track.title))?;

        let mut final_ext = "flac";
        if requested_quality == "HI_RES_LOSSLESS" {
            let is_flac = has_flac_stream_marker(&temp_path).await.map_err(|err| {
                format!("failed validating downloaded track {}: {err}", track.title)
            })?;
            if !is_flac {
                let container = sniff_media_container(&temp_path)
                    .await
                    .unwrap_or_else(|_| "unknown".to_string());
                if container == "mp4" {
                    final_ext = "m4a";
                    info!(
                        track_id = track.id,
                        album_id = job.album_id,
                        file = %temp_path.display(),
                        "HI_RES track is MP4 container with FLAC audio; keeping as .m4a"
                    );
                } else {
                    warn!(
                        track_id = track.id,
                        album_id = job.album_id,
                        file = %temp_path.display(),
                        container = %container,
                        "HI_RES output is not FLAC, retrying track in LOSSLESS"
                    );

                    let lossless_payload =
                        fetch_track_payload_for_quality(state, track.id, "LOSSLESS").await?;
                    download_payload_to_file(&state.http, &lossless_payload, &temp_path)
                        .await
                        .map_err(|err| {
                            format!("failed track {} in LOSSLESS fallback: {err}", track.title)
                        })?;

                    let fallback_is_flac =
                        has_flac_stream_marker(&temp_path).await.map_err(|err| {
                            format!(
                                "failed validating LOSSLESS fallback track {}: {err}",
                                track.title
                            )
                        })?;
                    if !fallback_is_flac {
                        return Err(format!(
                            "track {} is not FLAC even after LOSSLESS fallback",
                            track.title
                        ));
                    }
                }
            }
        }

        let final_path = album_dir.join(format!("{base_name}.{final_ext}"));

        fs::rename(&temp_path, &final_path)
            .await
            .map_err(|err| format!("failed to finalize track file: {err}"))?;

        if let Err(err) = write_audio_metadata(
            &final_path,
            &track.title,
            &track_artist,
            &artist_name,
            &job.album_title,
            track_number,
            disc_number,
            total_tracks as u32,
            &release_suffix,
            &track.extra,
            &album_extra,
            track_info_extra.as_ref(),
            lyrics.as_ref().and_then(|v| v.embedded_text.as_deref()),
            cover_art.as_deref(),
        ) {
            warn!(
                track_id = track.id,
                file = %final_path.display(),
                error = %err,
                "Skipping metadata write for track"
            );
        }

        if let Some(synced) = lyrics.as_ref().and_then(|v| v.synced_lrc.as_deref())
            && let Err(err) = write_lrc_sidecar(&final_path, synced).await
        {
            warn!(
                track_id = track.id,
                file = %final_path.display(),
                error = %err,
                "Skipping LRC sidecar write"
            );
        }

        update_job_progress(
            state,
            job.id,
            total_tracks,
            idx + 1,
            DownloadStatus::Downloading,
            None,
        )
        .await;
    }

    Ok(())
}

async fn update_job_progress(
    state: &AppState,
    job_id: u64,
    total_tracks: usize,
    completed_tracks: usize,
    status: DownloadStatus,
    error: Option<String>,
) {
    let mut jobs = state.download_jobs.write().await;
    if let Some(job) = jobs.iter_mut().find(|job| job.id == job_id) {
        job.total_tracks = total_tracks;
        job.completed_tracks = completed_tracks;
        job.status = status;
        job.error = error;
        job.updated_at = Utc::now();
        let _ = db::update_job(&state.db, job).await;
        debug!(
            job_id,
            album_id = job.album_id,
            status = job.status.as_str(),
            completed_tracks = job.completed_tracks,
            total_tracks = job.total_tracks,
            "Updated download progress"
        );
        state.notify_sse();
    }
}

enum DownloadPayload {
    DirectUrl(String),
    DashSegmentUrls(Vec<String>),
}

fn extract_download_payload(playback: &HifiPlaybackData) -> Result<DownloadPayload, String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(playback.manifest.as_bytes())
        .map_err(|err| format!("failed to decode manifest: {err}"))?;

    match playback.manifest_mime_type.as_str() {
        "application/vnd.tidal.bts" => {
            let manifest = serde_json::from_slice::<BtsManifest>(&decoded)
                .map_err(|err| format!("failed to parse BTS manifest: {err}"))?;
            manifest
                .urls
                .first()
                .cloned()
                .map(DownloadPayload::DirectUrl)
                .ok_or_else(|| "no track URL in BTS manifest".to_string())
        }
        "application/dash+xml" => {
            let xml = String::from_utf8(decoded)
                .map_err(|err| format!("DASH manifest is not valid UTF-8: {err}"))?;
            if let Ok(urls) = extract_dash_segment_urls(&xml)
                && !urls.is_empty()
            {
                return Ok(DownloadPayload::DashSegmentUrls(urls));
            }
            extract_dash_base_url(&xml).map(DownloadPayload::DirectUrl)
        }
        other => {
            warn!(manifest_mime_type = %other, "Unknown manifest type, attempting BTS parse as fallback");
            let manifest = serde_json::from_slice::<BtsManifest>(&decoded)
                .map_err(|err| format!("unsupported manifest type '{}': {err}", other))?;
            manifest
                .urls
                .first()
                .cloned()
                .map(DownloadPayload::DirectUrl)
                .ok_or_else(|| format!("no track URL in manifest (type: {})", other))
        }
    }
}

fn extract_dash_segment_urls(xml: &str) -> Result<Vec<String>, String> {
    let doc = Document::parse(xml).map_err(|err| format!("failed to parse DASH XML: {err}"))?;

    let mpd = doc
        .descendants()
        .find(|n| n.has_tag_name("MPD"))
        .ok_or_else(|| "DASH manifest has no MPD element".to_string())?;
    let period = mpd
        .children()
        .find(|n| n.has_tag_name("Period"))
        .ok_or_else(|| "DASH manifest has no Period element".to_string())?;

    let adaptation_sets: Vec<Node<'_, '_>> = period
        .children()
        .filter(|n| n.has_tag_name("AdaptationSet"))
        .collect();
    if adaptation_sets.is_empty() {
        return Err("DASH manifest has no AdaptationSet".to_string());
    }

    let audio_set = adaptation_sets
        .iter()
        .copied()
        .find(|set| {
            set.attribute("mimeType")
                .map(|v| v.starts_with("audio"))
                .unwrap_or(false)
                || set
                    .attribute("contentType")
                    .map(|v| v.eq_ignore_ascii_case("audio"))
                    .unwrap_or(false)
        })
        .unwrap_or(adaptation_sets[0]);

    let mut reps: Vec<Node<'_, '_>> = audio_set
        .children()
        .filter(|n| n.has_tag_name("Representation"))
        .collect();
    if reps.is_empty() {
        return Err("DASH manifest has no Representation".to_string());
    }
    reps.sort_by_key(|rep| {
        rep.attribute("bandwidth")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    });
    reps.reverse();
    let rep = reps[0];

    let rep_id = rep.attribute("id").unwrap_or("");
    let segment_template = rep
        .children()
        .find(|n| n.has_tag_name("SegmentTemplate"))
        .or_else(|| {
            audio_set
                .children()
                .find(|n| n.has_tag_name("SegmentTemplate"))
        })
        .ok_or_else(|| "DASH manifest has no SegmentTemplate".to_string())?;

    let initialization = segment_template.attribute("initialization");
    let media = segment_template
        .attribute("media")
        .ok_or_else(|| "DASH SegmentTemplate has no media template".to_string())?;
    let start_number = segment_template
        .attribute("startNumber")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);

    let base_url = rep
        .children()
        .find(|n| n.has_tag_name("BaseURL"))
        .and_then(|n| n.text())
        .or_else(|| {
            audio_set
                .children()
                .find(|n| n.has_tag_name("BaseURL"))
                .and_then(|n| n.text())
        })
        .or_else(|| {
            period
                .children()
                .find(|n| n.has_tag_name("BaseURL"))
                .and_then(|n| n.text())
        })
        .or_else(|| {
            mpd.children()
                .find(|n| n.has_tag_name("BaseURL"))
                .and_then(|n| n.text())
        })
        .unwrap_or("")
        .trim()
        .to_string();

    let timeline = segment_template
        .children()
        .find(|n| n.has_tag_name("SegmentTimeline"))
        .ok_or_else(|| "DASH SegmentTemplate has no SegmentTimeline".to_string())?;

    let mut entries = Vec::new();
    let mut current_time = 0u64;
    let mut current_number = start_number;
    for s in timeline.children().filter(|n| n.has_tag_name("S")) {
        if let Some(t) = s.attribute("t").and_then(|v| v.parse::<u64>().ok()) {
            current_time = t;
        }
        let duration = s
            .attribute("d")
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| "DASH timeline entry missing duration".to_string())?;
        let repeats = s
            .attribute("r")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);

        entries.push((current_number, current_time));
        current_number += 1;
        current_time += duration;

        for _ in 0..repeats.max(0) {
            entries.push((current_number, current_time));
            current_number += 1;
            current_time += duration;
        }
    }

    let mut urls = Vec::with_capacity(entries.len() + 1);
    if let Some(init) = initialization {
        let init_path = resolve_dash_template(init, rep_id, 0, 0);
        urls.push(join_dash_url(&base_url, &init_path));
    }
    for (number, time) in entries {
        let path = resolve_dash_template(media, rep_id, number, time);
        urls.push(join_dash_url(&base_url, &path));
    }

    if urls.is_empty() {
        return Err("DASH generated no segment URLs".to_string());
    }

    Ok(urls)
}

fn resolve_dash_template(template: &str, rep_id: &str, number: u64, time: u64) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let Some(end_rel) = template[i + 1..].find('$') else {
            out.push('$');
            i += 1;
            continue;
        };
        let end = i + 1 + end_rel;
        let token = &template[i + 1..end];
        if token == "RepresentationID" {
            out.push_str(rep_id);
        } else if let Some(width) = token
            .strip_prefix("Number%0")
            .and_then(|s| s.strip_suffix('d'))
        {
            let w = width.parse::<usize>().unwrap_or(0);
            if w > 0 {
                out.push_str(&format!("{number:0w$}"));
            } else {
                out.push_str(&number.to_string());
            }
        } else if token == "Number" {
            out.push_str(&number.to_string());
        } else if let Some(width) = token
            .strip_prefix("Time%0")
            .and_then(|s| s.strip_suffix('d'))
        {
            let w = width.parse::<usize>().unwrap_or(0);
            if w > 0 {
                out.push_str(&format!("{time:0w$}"));
            } else {
                out.push_str(&time.to_string());
            }
        } else if token == "Time" {
            out.push_str(&time.to_string());
        } else {
            out.push('$');
            out.push_str(token);
            out.push('$');
        }
        i = end + 1;
    }
    out
}

fn join_dash_url(base: &str, part: &str) -> String {
    if part.starts_with("http://") || part.starts_with("https://") {
        return part.to_string();
    }
    if base.is_empty() {
        return part.to_string();
    }
    if base.ends_with('/') || part.starts_with('/') {
        format!("{base}{part}")
    } else {
        format!("{base}/{part}")
    }
}

/// Extract the download URL from a DASH MPD XML manifest.
/// TIDAL DASH manifests contain `<BaseURL>` elements with the direct stream URL.
fn extract_dash_base_url(xml: &str) -> Result<String, String> {
    // Handles both <BaseURL>...</BaseURL> and <BaseURL attr="...">...</BaseURL>
    // and works regardless of line breaks/indentation.
    let mut scan_from = 0usize;
    while let Some(tag_start_rel) = xml[scan_from..].find("<BaseURL") {
        let tag_start = scan_from + tag_start_rel;
        let after_open = &xml[tag_start..];
        let Some(open_end_rel) = after_open.find('>') else {
            break;
        };
        let content_start = tag_start + open_end_rel + 1;

        let after_content = &xml[content_start..];
        let Some(close_rel) = after_content.find("</BaseURL>") else {
            scan_from = content_start;
            continue;
        };
        let raw = after_content[..close_rel].trim();
        let url = raw
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#x2F;", "/");

        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(url);
        }

        scan_from = content_start + close_rel + "</BaseURL>".len();
    }

    // Last resort: pick first absolute URL anywhere in the XML payload.
    if let Some(start) = xml.find("https://").or_else(|| xml.find("http://")) {
        let tail = &xml[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '<' || c == '"')
            .unwrap_or(tail.len());
        let candidate = tail[..end].trim();
        if candidate.starts_with("http://") || candidate.starts_with("https://") {
            return Ok(candidate.to_string());
        }
    }

    Err("no absolute URL found in DASH manifest".to_string())
}

fn summarize_manifest_for_logs(playback: &HifiPlaybackData) -> String {
    let decoded =
        match base64::engine::general_purpose::STANDARD.decode(playback.manifest.as_bytes()) {
            Ok(bytes) => bytes,
            Err(err) => return format!("decode_error={err}"),
        };

    if playback.manifest_mime_type != "application/dash+xml" {
        return format!(
            "mime_type={}, decoded_bytes={}",
            playback.manifest_mime_type,
            decoded.len()
        );
    }

    let xml = match String::from_utf8(decoded) {
        Ok(xml) => xml,
        Err(err) => return format!("dash_utf8_error={err}"),
    };

    let base_url_count = xml.matches("<BaseURL").count();
    let representation_count = xml.matches("<Representation").count();
    let adaptation_set_count = xml.matches("<AdaptationSet").count();
    let segment_template_count = xml.matches("<SegmentTemplate").count();
    let segment_base_count = xml.matches("<SegmentBase").count();
    let segment_list_count = xml.matches("<SegmentList").count();
    let content_protection_count = xml.matches("<ContentProtection").count();

    format!(
        "mime_type=application/dash+xml, xml_bytes={}, base_url={}, representation={}, adaptation_set={}, segment_template={}, segment_base={}, segment_list={}, content_protection={}",
        xml.len(),
        base_url_count,
        representation_count,
        adaptation_set_count,
        segment_template_count,
        segment_base_count,
        segment_list_count,
        content_protection_count
    )
}

async fn download_to_file(http: &reqwest::Client, url: &str, path: &PathBuf) -> Result<(), String> {
    let mut response = http
        .get(url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|err| format!("download request failed: {err}"))?
        .error_for_status()
        .map_err(|err| format!("download status failed: {err}"))?;

    let mut file = fs::File::create(path)
        .await
        .map_err(|err| format!("failed creating file: {err}"))?;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("failed reading stream chunk: {err}"))?
    {
        if chunk.is_empty() {
            continue;
        }
        for slice in chunk.chunks(DOWNLOAD_CHUNK_SIZE) {
            file.write_all(slice)
                .await
                .map_err(|err| format!("failed writing file: {err}"))?;
        }
    }

    file.flush()
        .await
        .map_err(|err| format!("failed flushing file: {err}"))?;
    Ok(())
}

async fn download_payload_to_file(
    http: &reqwest::Client,
    payload: &DownloadPayload,
    path: &PathBuf,
) -> Result<(), String> {
    match payload {
        DownloadPayload::DirectUrl(url) => download_to_file(http, url, path).await,
        DownloadPayload::DashSegmentUrls(urls) => {
            download_dash_segments_to_file(http, urls, path).await
        }
    }
}

async fn download_dash_segments_to_file(
    http: &reqwest::Client,
    urls: &[String],
    path: &PathBuf,
) -> Result<(), String> {
    let mut file = fs::File::create(path)
        .await
        .map_err(|err| format!("failed creating file: {err}"))?;

    for (idx, url) in urls.iter().enumerate() {
        let bytes = http
            .get(url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|err| format!("dash segment request failed at {idx}: {err}"))?
            .error_for_status()
            .map_err(|err| format!("dash segment status failed at {idx}: {err}"))?
            .bytes()
            .await
            .map_err(|err| format!("dash segment body failed at {idx}: {err}"))?;

        if bytes.is_empty() {
            continue;
        }
        for slice in bytes.chunks(DOWNLOAD_CHUNK_SIZE) {
            file.write_all(slice)
                .await
                .map_err(|err| format!("failed writing dash segment {idx}: {err}"))?;
        }
    }

    file.flush()
        .await
        .map_err(|err| format!("failed flushing file: {err}"))?;
    Ok(())
}

async fn has_flac_stream_marker(path: &PathBuf) -> Result<bool, String> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|err| format!("failed opening file {}: {err}", path.display()))?;
    let mut header = [0u8; 4];
    let read = file
        .read(&mut header)
        .await
        .map_err(|err| format!("failed reading header {}: {err}", path.display()))?;
    Ok(read == 4 && header == *b"fLaC")
}

async fn sniff_media_container(path: &PathBuf) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|err| format!("failed opening file {}: {err}", path.display()))?;
    let mut header = [0u8; 12];
    let read = file
        .read(&mut header)
        .await
        .map_err(|err| format!("failed reading header {}: {err}", path.display()))?;
    if read >= 4 && header[..4] == *b"fLaC" {
        return Ok("flac".to_string());
    }
    if read >= 8 && header[4..8] == *b"ftyp" {
        return Ok("mp4".to_string());
    }
    Ok("unknown".to_string())
}

async fn fetch_track_payload_for_quality(
    state: &AppState,
    track_id: i64,
    quality: &str,
) -> Result<DownloadPayload, String> {
    let playback = hifi_get_json::<HifiPlaybackResponse>(
        state,
        "/track/",
        vec![
            ("id".to_string(), track_id.to_string()),
            ("quality".to_string(), quality.to_string()),
        ],
    )
    .await?;

    extract_download_payload(&playback.data)
}

pub(crate) fn sanitize_path_component(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string();

    if sanitized.is_empty() {
        "Unknown".to_string()
    } else {
        sanitized
    }
}

fn write_audio_metadata(
    path: &PathBuf,
    title: &str,
    track_artist: &str,
    album_artist: &str,
    album: &str,
    track_number: u32,
    disc_number: Option<u32>,
    total_tracks: u32,
    release_date: &str,
    track_extra: &std::collections::HashMap<String, Value>,
    album_extra: &std::collections::HashMap<String, Value>,
    track_info_extra: Option<&std::collections::HashMap<String, Value>>,
    lyrics_text: Option<&str>,
    cover_art_jpeg: Option<&[u8]>,
) -> Result<(), String> {
    let default_tag_type = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("m4a") | Some("mp4") => TagType::Mp4Ilst,
        _ => TagType::VorbisComments,
    };

    let mut tagged_file = Probe::open(path)
        .map_err(|err| err.to_string())?
        .read()
        .map_err(|err| err.to_string())?;

    let tag = if let Some(existing) = tagged_file.primary_tag_mut() {
        existing
    } else {
        tagged_file.insert_tag(Tag::new(default_tag_type));
        tagged_file
            .primary_tag_mut()
            .ok_or_else(|| "failed to create metadata tag".to_string())?
    };

    tag.set_title(title.to_string());
    tag.set_artist(track_artist.to_string());
    tag.set_album(album.to_string());
    if !album_artist.trim().is_empty() {
        tag.insert_text(ItemKey::AlbumArtist, album_artist.to_string());
    }
    tag.insert_text(ItemKey::TrackNumber, track_number.to_string());
    if let Some(disc) = disc_number {
        tag.insert_text(ItemKey::DiscNumber, disc.to_string());
    }
    if total_tracks > 0 {
        tag.insert_text(ItemKey::TrackTotal, total_tracks.to_string());
    }
    let year = extract_year(release_date);
    if !year.is_empty() {
        tag.insert_text(ItemKey::Year, year);
    }
    if let Some(lyrics) = lyrics_text.filter(|v| !v.trim().is_empty()) {
        tag.insert_text(ItemKey::Lyrics, lyrics.to_string());
    }

    if let Some(info) = track_info_extra {
        if let Some(isrc) = value_as_string(info.get("isrc")) {
            tag.insert_text(ItemKey::Isrc, isrc);
        }
        if let Some(copyright) = value_as_string(info.get("copyright")) {
            tag.insert_text(ItemKey::CopyrightMessage, copyright);
        }
        if let Some(version) = value_as_string(info.get("version"))
            && !version.trim().is_empty()
        {
            tag.insert_text(ItemKey::TrackSubtitle, version);
        }
        if let Some(initial_key) = value_as_string(info.get("key")) {
            tag.insert_text(ItemKey::InitialKey, initial_key);
        }
        if let Some(bpm) = value_as_string(info.get("bpm")) {
            tag.insert_text(ItemKey::IntegerBpm, bpm);
        }
        if let Some(track_gain) = value_as_string(info.get("trackReplayGain")) {
            tag.insert_text(ItemKey::ReplayGainTrackGain, track_gain);
        }
        if let Some(track_peak) = value_as_string(info.get("trackPeakAmplitude")) {
            tag.insert_text(ItemKey::ReplayGainTrackPeak, track_peak);
        }
        if let Some(album_gain) = value_as_string(info.get("albumReplayGain")) {
            tag.insert_text(ItemKey::ReplayGainAlbumGain, album_gain);
        }
        if let Some(album_peak) = value_as_string(info.get("albumPeakAmplitude")) {
            tag.insert_text(ItemKey::ReplayGainAlbumPeak, album_peak);
        }
    }

    if let Some(jpeg) = cover_art_jpeg {
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(Picture::new_unchecked(
            PictureType::CoverFront,
            Some(MimeType::Jpeg),
            None,
            jpeg.to_vec(),
        ));
    }

    if default_tag_type == TagType::VorbisComments {
        write_extra_vorbis(tag, "TIDAL_TRACK_", track_extra);
        write_extra_vorbis(tag, "TIDAL_ALBUM_", album_extra);
        if let Some(info) = track_info_extra {
            write_extra_vorbis(tag, "TIDAL_INFO_", info);
        }
    }

    tagged_file
        .save_to_path(path, WriteOptions::default())
        .map_err(|err| err.to_string())
}

fn write_extra_vorbis(
    tag: &mut Tag,
    prefix: &str,
    extra: &std::collections::HashMap<String, Value>,
) {
    for (key, value) in extra {
        if let Some(text) = value_to_text(value) {
            let key = sanitize_vorbis_key(prefix, key);
            if !key.is_empty() {
                tag.insert_text(ItemKey::Unknown(key), text);
            }
        }
    }
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).ok(),
    }
}

fn value_as_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

fn build_full_artist_string(
    title: &str,
    track_extra: &HashMap<String, Value>,
    track_info_extra: Option<&HashMap<String, Value>>,
    fallback_artist: &str,
) -> String {
    let mut artists = Vec::<String>::new();
    let mut seen = std::collections::HashSet::<String>::new();

    let mut push_artist = |name: &str| push_unique_artist(name, &mut artists, &mut seen);

    collect_artists_from_map(track_extra, &mut push_artist);
    if let Some(extra) = track_info_extra {
        collect_artists_from_map(extra, &mut push_artist);
    }
    for featured in parse_featured_artists(title) {
        push_artist(&featured);
    }

    drop(push_artist);

    if artists.is_empty() {
        push_unique_artist(fallback_artist, &mut artists, &mut seen);
    }

    artists.join("; ")
}

fn push_unique_artist(
    name: &str,
    artists: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = trimmed.to_ascii_lowercase();
    if seen.insert(key) {
        artists.push(trimmed.to_string());
    }
}

fn collect_artists_from_map(map: &HashMap<String, Value>, push: &mut dyn FnMut(&str)) {
    for key in ["artists", "artist"] {
        if let Some(value) = map.get(key) {
            collect_artist_names(value, push);
        }
    }
}

fn collect_artist_names(value: &Value, push: &mut dyn FnMut(&str)) {
    match value {
        Value::String(s) => push(s),
        Value::Array(items) => {
            for item in items {
                collect_artist_names(item, push);
            }
        }
        Value::Object(obj) => {
            if let Some(Value::String(name)) = obj.get("name") {
                push(name);
            } else if let Some(Value::String(name)) = obj.get("title") {
                push(name);
            }
        }
        _ => {}
    }
}

fn parse_featured_artists(title: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let lower = title.to_ascii_lowercase();
    while let Some(open_rel) = lower[start..].find('(') {
        let open = start + open_rel;
        let Some(close_rel) = lower[open + 1..].find(')') else {
            break;
        };
        let close = open + 1 + close_rel;
        let inner = title[open + 1..close].trim();
        let inner_lower = inner.to_ascii_lowercase();
        let markers = ["feat.", "feat", "ft.", "ft", "with "];
        if let Some(marker) = markers.iter().find(|m| inner_lower.starts_with(**m)) {
            let raw = inner[marker.len()..].trim();
            for piece in raw.split(',') {
                for p in piece.split('&') {
                    let name = p.trim();
                    if !name.is_empty() {
                        out.push(name.to_string());
                    }
                }
            }
        }
        start = close + 1;
    }
    out
}

fn extract_disc_number(
    track_extra: &HashMap<String, Value>,
    track_info_extra: Option<&HashMap<String, Value>>,
) -> Option<u32> {
    for key in ["volumeNumber", "discNumber", "volume_number", "disc_number"] {
        if let Some(val) = track_info_extra
            .and_then(|m| m.get(key))
            .or_else(|| track_extra.get(key))
        {
            match val {
                Value::Number(n) => {
                    if let Some(v) = n.as_u64().and_then(|v| u32::try_from(v).ok()) {
                        return Some(v);
                    }
                }
                Value::String(s) => {
                    if let Ok(v) = s.trim().parse::<u32>() {
                        return Some(v);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn sanitize_vorbis_key(prefix: &str, key: &str) -> String {
    let normalized = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();

    format!("{}{}", prefix, normalized)
}

fn parse_track_number_from_path(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?.trim();
    let digits = stem
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

async fn fetch_cover_art_bytes(http: &reqwest::Client, cover_id: Option<&str>) -> Option<Vec<u8>> {
    let cover_id = cover_id?;
    let url = format!(
        "https://resources.tidal.com/images/{}/1080x1080.jpg",
        cover_id.replace('-', "/")
    );

    let resp = http
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;

    resp.bytes().await.ok().map(|b| b.to_vec())
}

async fn fetch_track_info_extra(
    state: &AppState,
    track_id: i64,
) -> Option<std::collections::HashMap<String, Value>> {
    let response = hifi_get_json::<Value>(
        state,
        "/info/",
        vec![("id".to_string(), track_id.to_string())],
    )
    .await
    .ok()?;

    let data = response.get("data")?.as_object()?;
    Some(data.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

struct LyricsBundle {
    embedded_text: Option<String>,
    synced_lrc: Option<String>,
}

async fn fetch_track_lyrics(
    state: &AppState,
    track_title: &str,
    artist_name: &str,
    album_title: &str,
    duration_secs: Option<u32>,
) -> Option<LyricsBundle> {
    let api = LRCLibAPI::new();
    let request = api
        .get_lyrics(
            track_title,
            artist_name,
            Some(album_title),
            duration_secs.map(u64::from),
        )
        .ok()?;

    let mut req = state.http.get(request.uri().to_string());
    if let Some(ua) = request
        .headers()
        .get("User-Agent")
        .and_then(|h| h.to_str().ok())
    {
        req = req.header(reqwest::header::USER_AGENT, ua.to_string());
    }

    let response = req.timeout(Duration::from_secs(10)).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }

    let payload = response.json::<GetLyricsResponse>().await.ok()?;
    let GetLyricsResponse::Success(data) = payload else {
        return None;
    };

    let plain = data
        .plain_lyrics
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let synced = data
        .synced_lyrics
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let embedded_text = if plain.is_some() {
        plain
    } else {
        synced.as_deref().map(strip_lrc_timestamps)
    };

    if embedded_text.is_none() && synced.is_none() {
        return None;
    }

    Some(LyricsBundle {
        embedded_text,
        synced_lrc: synced,
    })
}

fn strip_lrc_timestamps(input: &str) -> String {
    let mut out = Vec::new();
    for raw_line in input.lines() {
        let mut line = raw_line.trim();
        while line.starts_with('[') {
            let Some(end) = line.find(']') else {
                break;
            };
            let tag = &line[1..end];
            if tag
                .chars()
                .any(|c| c.is_ascii_digit() || c == ':' || c == '.')
            {
                line = line[end + 1..].trim_start();
                continue;
            }
            break;
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

async fn write_lrc_sidecar(audio_path: &Path, synced_lrc: &str) -> Result<(), String> {
    let sidecar_path = audio_path.with_extension("lrc");
    fs::write(&sidecar_path, synced_lrc)
        .await
        .map_err(|err| format!("failed writing sidecar {}: {err}", sidecar_path.display()))
}

fn extract_year(release_date: &str) -> String {
    let year = release_date.chars().take(4).collect::<String>();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        year
    } else {
        String::new()
    }
}

fn normalize_quality(raw: &str) -> String {
    let upper = raw.trim().to_ascii_uppercase();
    match upper.as_str() {
        "HI_RES_LOSSLESS" | "HI_RES_LOSLESS" | "HIRES_LOSSLESS" | "HIRES" => {
            "HI_RES_LOSSLESS".to_string()
        }
        "LOSSLESS" | "HIGH" | "LOW" => upper,
        _ => {
            warn!(quality = %raw, normalized = "LOSSLESS", "Unknown quality requested, using LOSSLESS");
            "LOSSLESS".to_string()
        }
    }
}
