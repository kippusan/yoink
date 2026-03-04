use std::sync::Arc;

use chrono::Utc;
use tokio::fs;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};
use yoink_shared::Quality;

use crate::{
    db,
    error::{AppError, AppResult},
    models::{DownloadJob, DownloadStatus},
    providers::{
        DownloadSource, DownloadTrackContext, MetadataProvider, PlaybackInfo, ProviderTrack,
    },
    state::AppState,
};

use super::io::{has_flac_stream_marker, sanitize_path_component, sniff_media_container};
use super::lyrics::{fetch_track_lyrics, write_lrc_sidecar};
use super::metadata::{
    TrackMetadata, build_full_artist_string, extract_disc_number, write_audio_metadata,
};

pub(crate) async fn download_album_job(state: &AppState, job: DownloadJob) -> AppResult<()> {
    let requested_quality = job.quality;

    // Resolve the provider link for this album to find the external ID and provider
    let album_links = db::load_album_provider_links(&state.db, job.album_id).await?;

    let download_source = state
        .registry
        .download_source(&job.source)
        .ok_or_else(|| {
            AppError::unavailable("download source", format!("'{}' not available", job.source))
        })?;

    // Pick a metadata provider link independently from the download source.
    // If the source itself has metadata, prefer it; otherwise use highest-priority linked metadata.
    let metadata_link = album_links
        .iter()
        .find(|l| {
            l.provider == job.source && state.registry.metadata_provider(&l.provider).is_some()
        })
        .or_else(|| {
            album_links
                .iter()
                .filter(|l| state.registry.metadata_provider(&l.provider).is_some())
                .max_by_key(|l| metadata_provider_priority(&l.provider))
        })
        .ok_or_else(|| {
            AppError::not_found("metadata provider link", Some(job.album_id.to_string()))
        })?;

    let metadata_provider = state
        .registry
        .metadata_provider(&metadata_link.provider)
        .ok_or_else(|| {
            AppError::unavailable(
                "metadata provider",
                format!("'{}' not available", metadata_link.provider),
            )
        })?;

    let external_album_id = &metadata_link.external_id;

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
        .await?;

    if provider_tracks.is_empty() {
        return Err(AppError::not_found(
            "downloadable tracks",
            Some(job.album_id.to_string()),
        ));
    }

    // Save the full album track count for metadata tagging (track N of M).
    let full_album_track_count = provider_tracks.len();

    // When the album is not fully monitored (partially_wanted), filter to only
    // the individually monitored tracks. Matching is done by ISRC or
    // (disc, track_number) position.
    let album_is_fully_monitored = {
        let albums = state.monitored_albums.read().await;
        albums
            .iter()
            .find(|a| a.id == job.album_id)
            .map(|a| a.monitored)
            .unwrap_or(false)
    };

    let provider_tracks = if album_is_fully_monitored {
        provider_tracks
    } else {
        // Load monitored tracks from DB to know which ones to download
        let monitored_tracks = db::load_monitored_tracks_for_album(&state.db, job.album_id)
            .await
            .unwrap_or_default();

        if monitored_tracks.is_empty() {
            return Err(AppError::not_found(
                "monitored tracks",
                Some(job.album_id.to_string()),
            ));
        }

        provider_tracks
            .into_iter()
            .filter(|pt| {
                monitored_tracks.iter().any(|mt| {
                    // Match by ISRC first (most reliable)
                    if let (Some(pt_isrc), Some(mt_isrc)) = (&pt.isrc, &mt.isrc)
                        && pt_isrc.eq_ignore_ascii_case(mt_isrc)
                    {
                        return true;
                    }
                    // Fallback: match by disc + track number
                    let pt_disc =
                        super::metadata::extract_disc_number(&pt.extra, None).unwrap_or(1);
                    mt.disc_number == pt_disc && mt.track_number == pt.track_number
                })
            })
            .collect::<Vec<_>>()
    };

    if provider_tracks.is_empty() {
        return Err(AppError::not_found(
            "matching provider tracks",
            Some(job.album_id.to_string()),
        ));
    }

    let total_tracks = provider_tracks.len();
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

    // Fetch cover art via the selected metadata provider
    let cover_art = if let Some(cover_ref) = metadata_link.cover_ref.as_deref() {
        metadata_provider.fetch_cover_art_bytes(cover_ref).await
    } else {
        None
    };

    let artist_dir = state.music_root.join(sanitize_path_component(artist_name));
    let album_dir = artist_dir.join(sanitize_path_component(&format!(
        "{} ({})",
        job.album_title, release_suffix
    )));
    fs::create_dir_all(&album_dir)
        .await
        .map_err(|err| {
            AppError::filesystem("create output directory", album_dir.display().to_string(), err)
        })?;

    let max_parallel = state.download_max_parallel_tracks.max(1);
    info!(
        job_id = %job.id,
        album_id = %job.album_id,
        max_parallel_tracks = max_parallel,
        "Downloading tracks with configured parallelism"
    );

    let mut completed_tracks = 0usize;
    let mut join_set = JoinSet::new();
    let mut track_iter = provider_tracks.into_iter();
    let mut in_flight = 0usize;

    while in_flight < max_parallel {
        let Some(track) = track_iter.next() else {
            break;
        };
        in_flight += 1;

        let state_clone = state.clone();
        let job_clone = job.clone();
        let requested_quality_clone = requested_quality;
        let download_source_clone = Arc::clone(&download_source);
        let metadata_provider_clone = Arc::clone(&metadata_provider);
        let metadata_provider_id = metadata_provider.id().to_string();
        let artist_name_owned = artist_name.to_string();
        let release_suffix_owned = release_suffix.clone();
        let album_dir_clone = album_dir.clone();
        let album_extra_clone = album_extra.clone();
        let cover_art_clone = cover_art.clone();

        let full_count = full_album_track_count;
        join_set.spawn(async move {
            process_track_download(
                state_clone,
                job_clone,
                track,
                requested_quality_clone,
                download_source_clone,
                metadata_provider_clone,
                metadata_provider_id,
                artist_name_owned,
                release_suffix_owned,
                album_dir_clone,
                album_extra_clone,
                cover_art_clone,
                total_tracks,
                full_count,
            )
            .await
        });
    }

    while in_flight > 0 {
        let Some(result) = join_set.join_next().await else {
            break;
        };
        in_flight -= 1;

        match result {
            Ok(Ok(())) => {
                completed_tracks += 1;
                update_job_progress(
                    state,
                    job.id,
                    total_tracks,
                    completed_tracks,
                    DownloadStatus::Downloading,
                    None,
                )
                .await;

                if let Some(next_track) = track_iter.next() {
                    in_flight += 1;

                    let state_clone = state.clone();
                    let job_clone = job.clone();
                    let requested_quality_clone = requested_quality;
                    let download_source_clone = Arc::clone(&download_source);
                    let metadata_provider_clone = Arc::clone(&metadata_provider);
                    let metadata_provider_id = metadata_provider.id().to_string();
                    let artist_name_owned = artist_name.to_string();
                    let release_suffix_owned = release_suffix.clone();
                    let album_dir_clone = album_dir.clone();
                    let album_extra_clone = album_extra.clone();
                    let cover_art_clone = cover_art.clone();
                    let full_count = full_album_track_count;

                    join_set.spawn(async move {
                        process_track_download(
                            state_clone,
                            job_clone,
                            next_track,
                            requested_quality_clone,
                            download_source_clone,
                            metadata_provider_clone,
                            metadata_provider_id,
                            artist_name_owned,
                            release_suffix_owned,
                            album_dir_clone,
                            album_extra_clone,
                            cover_art_clone,
                            total_tracks,
                            full_count,
                        )
                        .await
                    });
                }
            }
            Ok(Err(err)) => return Err(err),
            Err(err) => {
                return Err(AppError::task_join(err.to_string()));
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_track_download(
    state: AppState,
    job: DownloadJob,
    track: ProviderTrack,
    requested_quality: Quality,
    download_source: Arc<dyn DownloadSource>,
    metadata_provider: Arc<dyn MetadataProvider>,
    metadata_provider_id: String,
    artist_name: String,
    release_suffix: String,
    album_dir: std::path::PathBuf,
    album_extra: std::collections::HashMap<String, serde_json::Value>,
    cover_art: Option<Vec<u8>>,
    _total_tracks: usize,
    // Full album track count for metadata tagging (track N of M).
    // May differ from the download count when only a subset is being downloaded.
    full_album_track_count: usize,
) -> AppResult<()> {
    let base_name = format!(
        "{:02} - {}",
        track.track_number,
        sanitize_path_component(&track.title)
    );

    if let Some(existing_path) = find_existing_track_file(&album_dir, &base_name).await {
        info!(
            track_id = %track.external_id,
            file = %existing_path.display(),
            "Skipping already downloaded track"
        );
        return Ok(());
    }

    debug!(
        job_id = %job.id,
        album_id = %job.album_id,
        track_id = %track.external_id,
        track_number = track.track_number,
        track_title = %track.title,
        "Resolving track playback"
    );

    let track_context = DownloadTrackContext {
        artist_name: artist_name.clone(),
        album_title: job.album_title.clone(),
        track_title: track.title.clone(),
        track_number: Some(track.track_number),
        album_track_count: Some(full_album_track_count),
        duration_secs: Some(track.duration_secs),
    };

    let (track_payload, resolved_source_id) = resolve_playback_with_fallback(
        &state,
        download_source.as_ref(),
        &metadata_provider_id,
        &track.external_id,
        &track.title,
        &requested_quality,
        &track_context,
    )
    .await?;

    if resolved_source_id != download_source.id() {
        info!(
            requested_source = %download_source.id(),
            resolved_source = %resolved_source_id,
            track_id = %track.external_id,
            track_title = %track.title,
            "Resolved playback via fallback source"
        );
    }

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
        &artist_name,
    );
    let disc_number = extract_disc_number(&track.extra, track_info_extra.as_ref());
    let lyrics = if state.download_lyrics {
        fetch_track_lyrics(
            &state,
            &track.title,
            &artist_name,
            &job.album_title,
            Some(track.duration_secs),
        )
        .await
    } else {
        None
    };

    let temp_path = album_dir.join(format!("{base_name}.part"));

    download_playback_to_file(&state.http, &track_payload, &temp_path)
        .await
        .map_err(|err| AppError::download("download track", format!("{}: {err}", track.title)))?;

    let mut final_ext = "flac";
    if requested_quality == Quality::HiRes {
        let is_flac = has_flac_stream_marker(&temp_path)
            .await
            .map_err(|err| {
                AppError::download("validate track format", format!("{}: {err}", track.title))
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

                let (lossless_payload, _) = resolve_playback_with_fallback(
                    &state,
                    download_source.as_ref(),
                    &metadata_provider_id,
                    &track.external_id,
                    &track.title,
                    &Quality::Lossless,
                    &track_context,
                )
                .await
                .map_err(|e| {
                    AppError::download("resolve lossless fallback", format!(
                        "Failed to resolve LOSSLESS playback for {}: {e}",
                        track.title
                    ))
                })?;

                download_playback_to_file(&state.http, &lossless_payload, &temp_path)
                    .await
                    .map_err(|err| {
                        AppError::download("download lossless fallback", format!(
                            "failed track {} in LOSSLESS fallback: {err}",
                            track.title
                        ))
                    })?;

                let fallback_is_flac = has_flac_stream_marker(&temp_path).await.map_err(|err| {
                    AppError::download("validate lossless fallback", format!(
                        "failed validating LOSSLESS fallback track {}: {err}",
                        track.title
                    ))
                })?;
                if !fallback_is_flac {
                    return Err(AppError::validation(
                        Some("audio_format"),
                        format!(
                        "track {} is not FLAC even after LOSSLESS fallback",
                        track.title
                        ),
                    ));
                }
            }
        }
    }

    let final_path = album_dir.join(format!("{base_name}.{final_ext}"));
    fs::rename(&temp_path, &final_path)
        .await
        .map_err(|err| {
            AppError::filesystem("finalize track file", final_path.display().to_string(), err)
        })?;

    if let Err(err) = write_audio_metadata(&TrackMetadata {
        path: &final_path,
        title: &track.title,
        track_artist: &track_artist,
        album_artist: &artist_name,
        album: &job.album_title,
        track_number,
        disc_number,
        total_tracks: full_album_track_count as u32,
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

    // Persist track with artist and file path info
    let relative_path = final_path
        .strip_prefix(&state.music_root)
        .unwrap_or(&final_path)
        .to_string_lossy()
        .to_string();
    let explicit = track.explicit;
    let local_track_id = if let Ok(Some(id)) =
        db::find_track_by_provider_link(&state.db, &metadata_provider_id, &track.external_id).await
    {
        id
    } else if let Some(ref isrc) = track.isrc {
        db::find_track_by_album_isrc(&state.db, job.album_id, isrc)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(uuid::Uuid::now_v7)
    } else {
        db::find_track_by_album_position(
            &state.db,
            job.album_id,
            disc_number.unwrap_or(1),
            track_number,
        )
        .await
        .ok()
        .flatten()
        .unwrap_or_else(uuid::Uuid::now_v7)
    };

    // Preserve existing monitored flag if the track already exists in DB.
    // For fully-monitored albums all tracks inherit album-level monitoring;
    // for partially-wanted albums only explicitly-monitored tracks are downloaded,
    // so existing flag is always correct.
    let existing_monitored = {
        let tracks = db::load_tracks_for_album(&state.db, job.album_id)
            .await
            .unwrap_or_default();
        tracks
            .iter()
            .find(|t| t.id == local_track_id)
            .map(|t| t.monitored)
            .unwrap_or(false)
    };

    let track_info = yoink_shared::TrackInfo {
        id: local_track_id,
        title: track.title.clone(),
        version: track.version.clone(),
        disc_number: disc_number.unwrap_or(1),
        track_number,
        duration_secs: track.duration_secs,
        duration_display: String::new(),
        isrc: track.isrc.clone(),
        explicit,
        track_artist: Some(track_artist.clone()),
        file_path: Some(relative_path),
        monitored: existing_monitored,
        acquired: true, // Just downloaded — mark as acquired
    };
    if let Err(e) = db::upsert_track(&state.db, &track_info, job.album_id).await {
        warn!(track_id = %local_track_id, error = %e, "Failed to persist downloaded track to DB");
    }
    if let Err(e) = db::upsert_track_provider_link(
        &state.db,
        local_track_id,
        &metadata_provider_id,
        &track.external_id,
    )
    .await
    {
        warn!(track_id = %local_track_id, error = %e, "Failed to persist track provider link");
    }

    Ok(())
}

async fn find_existing_track_file(
    album_dir: &std::path::Path,
    base_name: &str,
) -> Option<std::path::PathBuf> {
    for ext in ["flac", "m4a", "mp3", "ogg", "wav", "aac"] {
        let path = album_dir.join(format!("{base_name}.{ext}"));
        if tokio::fs::try_exists(&path).await.ok()? {
            return Some(path);
        }
    }
    None
}

pub(crate) async fn update_job_progress(
    state: &AppState,
    job_id: uuid::Uuid,
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
        if let Err(e) = db::update_job(&state.db, job).await {
            warn!(job_id = %job_id, error = %e, "Failed to persist job progress to DB");
        }
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
) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| {
                AppError::filesystem("create parent directory", parent.display().to_string(), e)
            })?;
    }

    use super::io::DownloadPayload;
    let io_payload = match payload {
        PlaybackInfo::DirectUrl(url) => DownloadPayload::DirectUrl(url.clone()),
        PlaybackInfo::SegmentUrls(urls) => DownloadPayload::DashSegmentUrls(urls.clone()),
        PlaybackInfo::LocalFile(local_path) => {
            tokio::fs::copy(local_path, path)
                .await
                .map_err(|e| {
                    AppError::filesystem("copy local file", local_path.display().to_string(), e)
                })?;
            return Ok(());
        }
    };
    super::io::download_payload_to_file(http, &io_payload, path).await
}

fn metadata_provider_priority(provider_id: &str) -> u8 {
    match provider_id {
        "tidal" => 10,
        "deezer" => 9,
        "musicbrainz" => 1,
        _ => 5,
    }
}

async fn resolve_playback_with_fallback(
    state: &AppState,
    primary_source: &dyn crate::providers::DownloadSource,
    metadata_provider_id: &str,
    external_track_id: &str,
    track_title: &str,
    quality: &Quality,
    context: &DownloadTrackContext,
) -> AppResult<(PlaybackInfo, String)> {
    match primary_source
        .resolve_playback(external_track_id, quality, Some(context))
        .await
    {
        Ok(payload) => return Ok((payload, primary_source.id().to_string())),
        Err(primary_err) => {
            warn!(
                source = %primary_source.id(),
                track_id = %external_track_id,
                track_title,
                error = %primary_err,
                "Primary download source failed to resolve playback; attempting fallback"
            );
        }
    }

    for source in state.registry.download_sources() {
        if source.id() == primary_source.id() {
            continue;
        }

        // Only try sources that can work without linked provider IDs,
        // or those that match the metadata provider we fetched tracks from.
        if source.requires_linked_provider() && source.id() != metadata_provider_id {
            continue;
        }

        match source
            .resolve_playback(external_track_id, quality, Some(context))
            .await
        {
            Ok(payload) => return Ok((payload, source.id().to_string())),
            Err(err) => {
                warn!(
                    source = %source.id(),
                    track_id = %external_track_id,
                    track_title,
                    error = %err,
                    "Fallback download source failed to resolve playback"
                );
            }
        }
    }

    Err(AppError::unavailable(
        "playback",
        format!(
            "failed to resolve playback for {track_title}: no source could resolve track"
        ),
    ))
}
