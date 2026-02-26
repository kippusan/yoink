use chrono::Utc;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::{
    db,
    models::{DownloadJob, DownloadStatus},
    providers::{PlaybackInfo, Quality},
    state::AppState,
};

use super::io::{
    has_flac_stream_marker, sanitize_path_component,
    sniff_media_container,
};
use super::lyrics::{fetch_track_lyrics, write_lrc_sidecar};
use super::metadata::{
    TrackMetadata, build_full_artist_string, extract_disc_number, write_audio_metadata,
};

pub(crate) async fn download_album_job(state: &AppState, job: DownloadJob) -> Result<(), String> {
    let requested_quality = Quality::from_str_lossy(&job.quality);

    // Resolve the provider link for this album to find the external ID and provider
    let album_links = db::load_album_provider_links(&state.db, &job.album_id)
        .await
        .map_err(|e| format!("failed to load album provider links: {e}"))?;

    // Find the download source matching the job's source field
    let source_link = album_links
        .iter()
        .find(|l| l.provider == job.source)
        .ok_or_else(|| {
            format!(
                "No {} provider link found for this album",
                job.source
            )
        })?;

    let download_source = state
        .registry
        .download_source(&job.source)
        .ok_or_else(|| format!("Download source '{}' not available", job.source))?;

    let metadata_provider = state
        .registry
        .metadata_provider(&job.source)
        .ok_or_else(|| format!("Metadata provider '{}' not available", job.source))?;

    let external_album_id = &source_link.external_id;

    info!(
        job_id = %job.id,
        album_id = %job.album_id,
        external_album_id = %external_album_id,
        source = %job.source,
        artist_name = %job.artist_name,
        quality = %requested_quality,
        "Starting album download"
    );

    let artist_name = &job.artist_name;

    // Fetch tracks from the metadata provider
    let (provider_tracks, album_extra) = metadata_provider
        .fetch_tracks(external_album_id)
        .await
        .map_err(|e| format!("Failed to fetch tracks: {}", e.0))?;

    if provider_tracks.is_empty() {
        return Err("Album has no downloadable tracks".to_string());
    }

    let total_tracks = provider_tracks.len();
    update_job_progress(
        state,
        &job.id,
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

    // Fetch cover art via the provider
    let cover_art = if let Some(cover_ref) = source_link.cover_ref.as_deref() {
        metadata_provider.fetch_cover_art_bytes(cover_ref).await
    } else {
        None
    };

    let artist_dir = state
        .music_root
        .join(sanitize_path_component(artist_name));
    let album_dir = artist_dir.join(sanitize_path_component(&format!(
        "{} ({})",
        job.album_title, release_suffix
    )));
    fs::create_dir_all(&album_dir)
        .await
        .map_err(|err| format!("failed to create output directory: {err}"))?;

    for (idx, track) in provider_tracks.iter().enumerate() {
        debug!(
            job_id = %job.id,
            album_id = %job.album_id,
            track_id = %track.external_id,
            track_number = track.track_number,
            track_title = %track.title,
            "Resolving track playback"
        );

        let track_payload = download_source
            .resolve_playback(&track.external_id, &requested_quality)
            .await
            .map_err(|e| format!("Failed to resolve playback for {}: {}", track.title, e.0))?;

        if matches!(track_payload, PlaybackInfo::SegmentUrls(_)) {
            info!(
                track_id = %track.external_id,
                album_id = %job.album_id,
                "Using segment download for track"
            );
        }

        let track_number = track.track_number;
        let track_info_extra = metadata_provider
            .fetch_track_info_extra(&track.external_id)
            .await;
        let track_artist = build_full_artist_string(
            &track.title,
            &track.extra,
            track_info_extra.as_ref(),
            artist_name,
        );
        let disc_number = extract_disc_number(&track.extra, track_info_extra.as_ref());
        let lyrics = if state.download_lyrics {
            fetch_track_lyrics(
                state,
                &track.title,
                artist_name,
                &job.album_title,
                Some(track.duration_secs),
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

        download_playback_to_file(&state.http, &track_payload, &temp_path)
            .await
            .map_err(|err| format!("failed track {}: {err}", track.title))?;

        let mut final_ext = "flac";
        if requested_quality == Quality::HiRes {
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
                        track_id = %track.external_id,
                        album_id = %job.album_id,
                        file = %temp_path.display(),
                        "HI_RES track is MP4 container with FLAC audio; keeping as .m4a"
                    );
                } else {
                    warn!(
                        track_id = %track.external_id,
                        album_id = %job.album_id,
                        file = %temp_path.display(),
                        container = %container,
                        "HI_RES output is not FLAC, retrying track in LOSSLESS"
                    );

                    let lossless_payload = download_source
                        .resolve_playback(&track.external_id, &Quality::Lossless)
                        .await
                        .map_err(|e| {
                            format!(
                                "Failed to resolve LOSSLESS playback for {}: {}",
                                track.title, e.0
                            )
                        })?;
                    download_playback_to_file(&state.http, &lossless_payload, &temp_path)
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
            album_artist: artist_name,
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
                track_id = %track.external_id,
                file = %final_path.display(),
                error = %err,
                "Skipping metadata write for track"
            );
        }

        if let Some(synced) = lyrics.as_ref().and_then(|v| v.synced_lrc.as_deref())
            && let Err(err) = write_lrc_sidecar(&final_path, synced).await
        {
            warn!(
                track_id = %track.external_id,
                file = %final_path.display(),
                error = %err,
                "Skipping LRC sidecar write"
            );
        }

        update_job_progress(
            state,
            &job.id,
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
    job_id: &str,
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
            job_id = %job_id,
            album_id = %job.album_id,
            status = job.status.as_str(),
            completed_tracks = job.completed_tracks,
            total_tracks = job.total_tracks,
            "Updated download progress"
        );
        state.notify_sse();
    }
}

/// Download a PlaybackInfo payload to a file using the io module.
async fn download_playback_to_file(
    http: &reqwest::Client,
    payload: &PlaybackInfo,
    path: &std::path::Path,
) -> Result<(), String> {
    use super::io::DownloadPayload;
    let io_payload = match payload {
        PlaybackInfo::DirectUrl(url) => DownloadPayload::DirectUrl(url.clone()),
        PlaybackInfo::SegmentUrls(urls) => DownloadPayload::DashSegmentUrls(urls.clone()),
    };
    super::io::download_payload_to_file(http, &io_payload, path).await
}
