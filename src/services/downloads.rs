use std::{collections::HashMap, path::PathBuf, time::Duration};

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
use tokio::{fs, io::AsyncWriteExt};
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
        debug!(album_id = album.id, "Skipping enqueue because active job exists");
        return;
    }

    let mut new_job = DownloadJob {
        id: 0, // will be set from DB
        album_id: album.id,
        artist_id: album.artist_id,
        album_title: album.title.clone(),
        status: DownloadStatus::Queued,
        quality: state.default_quality.clone(),
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
        quality = %state.default_quality,
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
            if let Some(job) = jobs.iter_mut().find(|job| job.status == DownloadStatus::Queued) {
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

        info!(job_id = job.id, album_id = job.album_id, "Processing download job");

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
                info!(job_id = job.id, album_id = job.album_id, "Download job completed");
                let mut albums = state.monitored_albums.write().await;
                if let Some(album) = albums.iter_mut().find(|album| album.id == job.album_id) {
                    album.acquired = true;
                    update_wanted(album);
                    let _ = db::update_album_flags(&state.db, album.id, album.monitored, album.acquired, album.wanted).await;
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
                    let _ = db::update_album_flags(&state.db, album.id, album.monitored, album.acquired, album.wanted).await;
                }
                state.notify_sse();
            }
        }
    }
}

pub(crate) async fn retag_existing_files(state: &AppState) -> Result<(usize, usize, usize), String> {
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
        let album_dir = state.music_root.join(sanitize_path_component(artist_name)).join(
            sanitize_path_component(&format!("{} ({})", album.title, release_suffix)),
        );

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

        let mut entries = fs::read_dir(&album_dir)
            .await
            .map_err(|err| format!("failed to read album directory {}: {err}", album_dir.display()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| format!("failed to read directory entry {}: {err}", album_dir.display()))?
        {
            let path = entry.path();
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !ext.eq_ignore_ascii_case("flac") {
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
            let path = files_by_track
                .get(&track_number)
                .cloned()
                .or_else(|| ordered_files.get(idx).cloned());

            let Some(path) = path else {
                missing_files += 1;
                continue;
            };

            write_flac_metadata(
                &path,
                &track.title,
                artist_name,
                &album.title,
                track_number,
                total_tracks,
                &release_suffix,
                &track.extra,
                &album_extra,
                track_info_extra.as_ref(),
                cover_art.as_deref(),
            )?;
            tagged_files += 1;
        }
    }

    Ok((tagged_files, missing_files, scanned_albums))
}

async fn download_album_job(state: &AppState, job: DownloadJob) -> Result<(), String> {
    info!(
        job_id = job.id,
        album_id = job.album_id,
        artist_id = job.artist_id,
        quality = %job.quality,
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
    update_job_progress(state, job.id, total_tracks, 0, DownloadStatus::Downloading, None).await;

    let album = {
        let albums = state.monitored_albums.read().await;
        albums.iter().find(|album| album.id == job.album_id).cloned()
    };
    let release_suffix = album
        .as_ref()
        .and_then(|album| album.release_date.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let cover_art = fetch_cover_art_bytes(&state.http, album.as_ref().and_then(|a| a.cover.as_deref())).await;

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
                ("quality".to_string(), job.quality.clone()),
            ],
        )
        .await?;
        let track_url = match extract_download_url(&playback.data) {
            Ok(url) => url,
            Err(err)
                if playback.data.manifest_mime_type == "application/dash+xml"
                    && job.quality == "HI_RES_LOSSLESS" =>
            {
                warn!(
                    track_id = track.id,
                    album_id = job.album_id,
                    error = %err,
                    "HI_RES DASH manifest unsupported, falling back to LOSSLESS"
                );

                let lossless_playback = hifi_get_json::<HifiPlaybackResponse>(
                    state,
                    "/track/",
                    vec![
                        ("id".to_string(), track.id.to_string()),
                        ("quality".to_string(), "LOSSLESS".to_string()),
                    ],
                )
                .await?;

                extract_download_url(&lossless_playback.data)?
            }
            Err(err) => return Err(err),
        };
        let track_number = track.track_number.unwrap_or((idx + 1) as u32);
        let track_info_extra = fetch_track_info_extra(state, track.id).await;
        let file_name = format!(
            "{:02} - {}.flac",
            track_number,
            sanitize_path_component(&track.title)
        );
        let final_path = album_dir.join(file_name);
        let temp_path = final_path.with_extension("flac.part");

        download_to_file(&state.http, &track_url, &temp_path)
            .await
            .map_err(|err| format!("failed track {}: {err}", track.title))?;
        fs::rename(&temp_path, &final_path)
            .await
            .map_err(|err| format!("failed to finalize track file: {err}"))?;

        if let Err(err) = write_flac_metadata(
            &final_path,
            &track.title,
            &artist_name,
            &job.album_title,
            track_number,
            total_tracks as u32,
            &release_suffix,
            &track.extra,
            &album_extra,
            track_info_extra.as_ref(),
            cover_art.as_deref(),
        ) {
            warn!(
                track_id = track.id,
                file = %final_path.display(),
                error = %err,
                "Skipping metadata write for track"
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

fn extract_download_url(playback: &HifiPlaybackData) -> Result<String, String> {
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
                .ok_or_else(|| "no track URL in BTS manifest".to_string())
        }
        "application/dash+xml" => {
            // DASH MPD manifest — extract BaseURL from the XML
            let xml = String::from_utf8(decoded)
                .map_err(|err| format!("DASH manifest is not valid UTF-8: {err}"))?;
            extract_dash_base_url(&xml)
        }
        other => {
            warn!(manifest_mime_type = %other, "Unknown manifest type, attempting BTS parse as fallback");
            let manifest = serde_json::from_slice::<BtsManifest>(&decoded)
                .map_err(|err| format!("unsupported manifest type '{}': {err}", other))?;
            manifest
                .urls
                .first()
                .cloned()
                .ok_or_else(|| format!("no track URL in manifest (type: {})", other))
        }
    }
}

/// Extract the download URL from a DASH MPD XML manifest.
/// TIDAL DASH manifests contain `<BaseURL>` elements with the direct stream URL.
fn extract_dash_base_url(xml: &str) -> Result<String, String> {
    // Simple XML parsing — find <BaseURL>...</BaseURL>
    // We don't need a full XML parser for this; the format is predictable.
    for line in xml.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<BaseURL>") {
            if let Some(url) = rest.strip_suffix("</BaseURL>") {
                let url = url.trim();
                if url.starts_with("http") {
                    return Ok(url.to_string());
                }
            }
        }
    }
    // Fallback: regex-like scan for BaseURL tag anywhere in the document
    if let Some(start) = xml.find("<BaseURL>") {
        let after = &xml[start + 9..];
        if let Some(end) = after.find("</BaseURL>") {
            let url = after[..end].trim();
            if url.starts_with("http") {
                return Ok(url.to_string());
            }
        }
    }

    Err("no BaseURL found in DASH manifest".to_string())
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

fn write_flac_metadata(
    path: &PathBuf,
    title: &str,
    artist: &str,
    album: &str,
    track_number: u32,
    total_tracks: u32,
    release_date: &str,
    track_extra: &std::collections::HashMap<String, Value>,
    album_extra: &std::collections::HashMap<String, Value>,
    track_info_extra: Option<&std::collections::HashMap<String, Value>>,
    cover_art_jpeg: Option<&[u8]>,
) -> Result<(), String> {
    let mut tagged_file = Probe::open(path)
        .map_err(|err| err.to_string())?
        .read()
        .map_err(|err| err.to_string())?;

    let tag = if let Some(existing) = tagged_file.primary_tag_mut() {
        existing
    } else {
        tagged_file.insert_tag(Tag::new(TagType::VorbisComments));
        tagged_file
            .primary_tag_mut()
            .ok_or_else(|| "failed to create metadata tag".to_string())?
    };

    tag.set_title(title.to_string());
    tag.set_artist(artist.to_string());
    tag.set_album(album.to_string());
    tag.insert_text(ItemKey::TrackNumber, track_number.to_string());
    if total_tracks > 0 {
        tag.insert_text(ItemKey::TrackTotal, total_tracks.to_string());
    }
    let year = extract_year(release_date);
    if !year.is_empty() {
        tag.insert_text(ItemKey::Year, year);
    }

    if let Some(info) = track_info_extra {
        if let Some(isrc) = value_as_string(info.get("isrc")) {
            tag.insert_text(ItemKey::Isrc, isrc);
        }
        if let Some(copyright) = value_as_string(info.get("copyright")) {
            tag.insert_text(ItemKey::CopyrightMessage, copyright);
        }
        if let Some(version) = value_as_string(info.get("version")) {
            if !version.trim().is_empty() {
                tag.insert_text(ItemKey::TrackSubtitle, version);
            }
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

    write_extra_vorbis(tag, "TIDAL_TRACK_", track_extra);
    write_extra_vorbis(tag, "TIDAL_ALBUM_", album_extra);
    if let Some(info) = track_info_extra {
        write_extra_vorbis(tag, "TIDAL_INFO_", info);
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

fn parse_track_number_from_path(path: &PathBuf) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?.trim();
    let digits = stem.chars().take_while(|c| c.is_ascii_digit()).collect::<String>();
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

fn extract_year(release_date: &str) -> String {
    let year = release_date.chars().take(4).collect::<String>();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        year
    } else {
        String::new()
    }
}
