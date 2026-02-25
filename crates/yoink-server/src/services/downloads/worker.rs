use chrono::Utc;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::{
    db,
    models::{DownloadJob, DownloadStatus, HifiAlbumItem, HifiAlbumResponse, HifiPlaybackResponse},
    state::AppState,
};

use super::io::{
    download_payload_to_file, has_flac_stream_marker, normalize_quality, sanitize_path_component,
    sniff_media_container,
};
use super::lyrics::{fetch_track_lyrics, write_lrc_sidecar};
use super::manifest::{DownloadPayload, extract_download_payload, summarize_manifest_for_logs};
use super::metadata::{
    TrackMetadata, build_full_artist_string, extract_disc_number, fetch_cover_art_bytes,
    fetch_track_info_extra, write_audio_metadata,
};
use crate::services::hifi::hifi_get_json;

pub(crate) async fn download_album_job(state: &AppState, job: DownloadJob) -> Result<(), String> {
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

        if let Err(err) = write_audio_metadata(&TrackMetadata {
            path: &final_path,
            title: &track.title,
            track_artist: &track_artist,
            album_artist: &artist_name,
            album: &job.album_title,
            track_number,
            disc_number,
            total_tracks: total_tracks as u32,
            release_date: &release_suffix,
            track_extra: &track.extra,
            album_extra: &album_extra,
            track_info_extra: track_info_extra.as_ref(),
            lyrics_text: lyrics.as_ref().and_then(|v| v.embedded_text.as_deref()),
            cover_art_jpeg: cover_art.as_deref(),
        }) {
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

pub(crate) async fn update_job_progress(
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
