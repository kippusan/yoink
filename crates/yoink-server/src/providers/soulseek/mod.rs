use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, warn};

use super::{DownloadSource, DownloadTrackContext, PlaybackInfo, ProviderError};
use yoink_shared::Quality;

pub(crate) struct SoulSeekSource {
    http: reqwest::Client,
    slskd_base_url: String,
    username: String,
    password: String,
    downloads_dir: PathBuf,
    token: RwLock<Option<String>>,
    search_request_gate: Semaphore,
}

impl SoulSeekSource {
    pub fn new(
        http: reqwest::Client,
        slskd_base_url: String,
        username: String,
        password: String,
        downloads_dir: String,
    ) -> Self {
        Self {
            http,
            slskd_base_url: slskd_base_url.trim_end_matches('/').to_string(),
            username: username.trim().to_string(),
            password: password.trim().to_string(),
            downloads_dir: PathBuf::from(downloads_dir.trim()),
            token: RwLock::new(None),
            // slskd allows only one concurrent POST /searches operation.
            search_request_gate: Semaphore::new(1),
        }
    }

    async fn auth_token(&self) -> Result<Option<String>, ProviderError> {
        if self.username.is_empty() || self.password.is_empty() {
            return Ok(None);
        }

        if let Some(token) = self.token.read().await.clone() {
            return Ok(Some(token));
        }

        let url = format!("{}/api/v0/session", self.slskd_base_url);
        let payload = SlskdLoginRequest {
            username: self.username.clone(),
            password: self.password.clone(),
        };

        let resp = self
            .http
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError(format!("slskd login request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!(
                "slskd login failed with status {}",
                resp.status()
            )));
        }

        let token_resp: SlskdTokenResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError(format!("failed parsing slskd login response: {e}")))?;

        let token = token_resp.token;
        *self.token.write().await = Some(token.clone());
        Ok(Some(token))
    }

    async fn post_json<T: for<'de> Deserialize<'de>, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ProviderError> {
        let token = self.auth_token().await?;
        let url = format!("{}{}", self.slskd_base_url, path);
        let mut req = self
            .http
            .post(url)
            .json(body)
            .timeout(Duration::from_secs(30));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| ProviderError(format!("slskd POST {path} failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!(
                "slskd POST {path} returned {}",
                resp.status()
            )));
        }

        resp.json()
            .await
            .map_err(|e| ProviderError(format!("slskd POST {path} decode failed: {e}")))
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, ProviderError> {
        let token = self.auth_token().await?;
        let url = format!("{}{}", self.slskd_base_url, path);
        let mut req = self.http.get(url).timeout(Duration::from_secs(30));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| ProviderError(format!("slskd GET {path} failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!(
                "slskd GET {path} returned {}",
                resp.status()
            )));
        }

        resp.json()
            .await
            .map_err(|e| ProviderError(format!("slskd GET {path} decode failed: {e}")))
    }

    async fn start_search(&self, query: &str) -> Result<SlskdSearch, ProviderError> {
        let _permit = self
            .search_request_gate
            .acquire()
            .await
            .map_err(|_| ProviderError("soulseek search gate closed".to_string()))?;

        let req = SlskdSearchRequest {
            id: None,
            search_text: query.to_string(),
            // Let slskd defaults (and user-configured server options) decide limits.
            search_timeout: None,
            response_limit: None,
            file_limit: None,
        };

        // slskd returns 429 if a concurrent /searches request is in-flight.
        // Retry briefly with backoff to smooth bursts from parallel track jobs.
        let mut delay_secs = 1u64;
        for attempt in 1..=5 {
            match self.post_json("/api/v0/searches", &req).await {
                Ok(search) => return Ok(search),
                Err(err) if is_rate_limited_error(&err) && attempt < 5 => {
                    warn!(
                        query = %query,
                        attempt,
                        delay_secs,
                        "SoulSeek search creation rate-limited; retrying"
                    );
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    delay_secs = (delay_secs * 2).min(8);
                }
                Err(err) => return Err(err),
            }
        }

        Err(ProviderError(
            "SoulSeek search creation failed after retries".to_string(),
        ))
    }

    async fn search_with_fallback_queries(
        &self,
        ctx: &DownloadTrackContext,
    ) -> Result<Vec<SlskdSearchResponse>, ProviderError> {
        let mut queries = Vec::new();
        let artist = ctx.artist_name.trim();
        let album = ctx.album_title.trim();
        let track = ctx.track_title.trim();

        // Most precise to broadest.
        queries.push(format!("{artist} {album} {track}"));
        queries.push(format!("{artist} {track}"));
        queries.push(format!("{track} {artist}"));
        queries.push(format!("{track} {album}"));
        queries.push(track.to_string());

        // Add a normalized variant with punctuation removed for troublesome titles.
        let track_norm = normalize(track);
        if !track_norm.is_empty() && track_norm != track.to_ascii_lowercase() {
            queries.push(track_norm);
        }

        // Deduplicate while preserving order.
        let mut deduped = Vec::new();
        for q in queries {
            let q = q.trim().to_string();
            if q.is_empty() || deduped.iter().any(|existing: &String| existing == &q) {
                continue;
            }
            deduped.push(q);
        }

        for query in deduped {
            let search = self.start_search(&query).await?;
            let responses = self.poll_search_responses(&search.id, 75).await?;
            if !responses.is_empty() {
                debug!(
                    query = %query,
                    responses = responses.len(),
                    "SoulSeek search returned responses"
                );
                return Ok(responses);
            }
            debug!(query = %query, "SoulSeek search returned no responses");
        }

        Ok(Vec::new())
    }

    async fn search_album_queries(
        &self,
        ctx: &DownloadTrackContext,
        quality: &Quality,
    ) -> Result<Vec<SlskdSearchResponse>, ProviderError> {
        let expected_tracks = ctx.album_track_count.unwrap_or(0);
        if expected_tracks == 0 {
            return Ok(Vec::new());
        }

        let mut queries = Vec::new();
        let artist = ctx.artist_name.trim();
        let album = ctx.album_title.trim();
        if artist.is_empty() || album.is_empty() {
            return Ok(Vec::new());
        }

        queries.push(format!("{artist} {album}"));
        queries.push(format!("{album} {artist}"));
        let quality_hint = match quality {
            Quality::HiRes | Quality::Lossless => "flac",
            _ => "mp3",
        };
        queries.push(format!("{artist} {album} {quality_hint}"));

        let album_norm = normalize(album);
        if !album_norm.is_empty() && album_norm != album.to_ascii_lowercase() {
            queries.push(format!("{artist} {album_norm}"));
        }

        let mut deduped = Vec::new();
        for q in queries {
            let q = q.trim().to_string();
            if q.is_empty() || deduped.iter().any(|existing: &String| existing == &q) {
                continue;
            }
            deduped.push(q);
        }

        for query in deduped {
            let search = self.start_search(&query).await?;
            let responses = self.poll_search_responses(&search.id, 75).await?;
            if !responses.is_empty() {
                debug!(
                    query = %query,
                    responses = responses.len(),
                    "SoulSeek album search returned responses"
                );
                return Ok(responses);
            }
            debug!(query = %query, "SoulSeek album search returned no responses");
        }

        Ok(Vec::new())
    }

    async fn poll_search_responses(
        &self,
        search_id: &str,
        wait_secs: u64,
    ) -> Result<Vec<SlskdSearchResponse>, ProviderError> {
        let mut elapsed = 0u64;
        let state_path = format!("/api/v0/searches/{search_id}");
        let responses_path = format!("/api/v0/searches/{search_id}/responses");
        let mut saw_response_count = false;

        while elapsed < wait_secs {
            let status: SlskdSearchStatus = self.get_json(&state_path).await?;
            if status.response_count > 0 {
                saw_response_count = true;
                let responses: Vec<SlskdSearchResponse> = self.get_json(&responses_path).await?;
                if !responses.is_empty() {
                    return Ok(responses);
                }
            }

            if status.is_complete {
                if !saw_response_count {
                    return Ok(Vec::new());
                }

                // slskd may only materialize response payloads near completion.
                let responses: Vec<SlskdSearchResponse> = self.get_json(&responses_path).await?;
                return Ok(responses);
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
            elapsed += 2;
        }

        if saw_response_count {
            let responses: Vec<SlskdSearchResponse> = self.get_json(&responses_path).await?;
            return Ok(responses);
        }

        Ok(Vec::new())
    }

    async fn enqueue_download(
        &self,
        username: &str,
        filename: &str,
        size: i64,
    ) -> Result<(), ProviderError> {
        let path = format!(
            "/api/v0/transfers/downloads/{}",
            percent_encode_path(username)
        );
        let body = vec![SlskdQueueDownloadRequest {
            filename: filename.to_string(),
            size,
        }];

        let token = self.auth_token().await?;
        let url = format!("{}{}", self.slskd_base_url, path);
        let mut req = self
            .http
            .post(url)
            .json(&body)
            .timeout(Duration::from_secs(30));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| ProviderError(format!("slskd enqueue download failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ProviderError(format!(
                "slskd enqueue download returned {}",
                resp.status()
            )));
        }

        Ok(())
    }

    async fn wait_for_download_file(
        &self,
        username: &str,
        filename: &str,
        timeout_secs: u64,
    ) -> Result<PathBuf, ProviderError> {
        let path = format!(
            "/api/v0/transfers/downloads/{}",
            percent_encode_path(username)
        );
        let mut elapsed = 0u64;

        while elapsed < timeout_secs {
            let transfer_user: SlskdTransferUserResponse = self.get_json(&path).await?;
            let mut seen_matching_transfer = false;

            for dir in transfer_user.directories {
                for file in dir.files {
                    if file.filename == filename {
                        seen_matching_transfer = true;
                        if is_transfer_failure(&file) {
                            let detail = file
                                .exception
                                .clone()
                                .or_else(|| file.state_description.clone())
                                .unwrap_or_else(|| "unknown transfer failure".to_string());
                            return Err(ProviderError(format!(
                                "SoulSeek transfer failed for {filename}: {detail}"
                            )));
                        }

                        if is_transfer_complete_success(&file) {
                            for candidate in self.resolve_local_download_paths(
                                dir.directory.as_deref(),
                                &file.filename,
                            ) {
                                if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
                                    return Ok(candidate);
                                }
                            }
                        }
                    }
                }
            }

            if seen_matching_transfer {
                debug!(
                    username,
                    filename, "soulseek transfer found, waiting for completion"
                );
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
            elapsed += 2;
        }

        Err(ProviderError(format!(
            "Timed out waiting for soulseek download: {filename}"
        )))
    }

    fn resolve_local_download_paths(
        &self,
        directory: Option<&str>,
        slsk_filename: &str,
    ) -> Vec<PathBuf> {
        let mut out = Vec::new();

        let file_path = sanitize_relative_path(slsk_filename);
        out.push(self.downloads_dir.join(&file_path));
        if let Some(name) = Path::new(&file_path).file_name() {
            out.push(self.downloads_dir.join(name));
        }

        if let Some(dir) = directory {
            let dir_path = sanitize_relative_path(dir);
            if let Some(name) = Path::new(&file_path).file_name() {
                out.push(self.downloads_dir.join(&dir_path).join(name));
                if let Some(leaf) = Path::new(&dir_path).file_name() {
                    out.push(self.downloads_dir.join(leaf).join(name));
                }
            }
        }

        out.sort();
        out.dedup();
        out
    }
}

#[async_trait]
impl DownloadSource for SoulSeekSource {
    fn id(&self) -> &str {
        "soulseek"
    }

    fn requires_linked_provider(&self) -> bool {
        false
    }

    async fn resolve_playback(
        &self,
        external_track_id: &str,
        quality: &Quality,
        context: Option<&DownloadTrackContext>,
    ) -> Result<PlaybackInfo, ProviderError> {
        let ctx = context.ok_or_else(|| {
            ProviderError("SoulSeek requires track context for search/matching".to_string())
        })?;

        let album_responses = self.search_album_queries(ctx, quality).await?;
        let candidate = if album_responses.is_empty() {
            None
        } else {
            pick_track_from_complete_album_bundle(&album_responses, ctx, quality)
        };

        let candidate = match candidate {
            Some(c) => c,
            None => {
                let responses = self.search_with_fallback_queries(ctx).await?;
                if responses.is_empty() {
                    return Err(ProviderError(format!(
                        "No SoulSeek search responses for track '{}'",
                        ctx.track_title
                    )));
                }

                pick_best_candidate(&responses, ctx, quality).ok_or_else(|| {
                    ProviderError(format!(
                        "No suitable SoulSeek candidate for '{}'",
                        ctx.track_title
                    ))
                })?
            }
        };

        self.enqueue_download(&candidate.username, &candidate.filename, candidate.size)
            .await?;

        let local_path = self
            .wait_for_download_file(&candidate.username, &candidate.filename, 180)
            .await
            .map_err(|e| {
                warn!(
                    track_id = external_track_id,
                    username = candidate.username,
                    filename = candidate.filename,
                    error = %e,
                    "SoulSeek transfer did not complete in time"
                );
                e
            })?;

        Ok(PlaybackInfo::LocalFile(local_path))
    }
}

#[derive(Debug, Clone)]
struct SoulSeekCandidate {
    username: String,
    filename: String,
    size: i64,
    score: i32,
}

fn pick_best_candidate(
    responses: &[SlskdSearchResponse],
    ctx: &DownloadTrackContext,
    quality: &Quality,
) -> Option<SoulSeekCandidate> {
    let artist = normalize(&ctx.artist_name);
    let album = normalize(&ctx.album_title);
    let title = normalize(&ctx.track_title);

    let mut best: Option<SoulSeekCandidate> = None;

    for resp in responses {
        for file in &resp.files {
            let filename = normalize(&file.filename);
            let mut score = 0i32;

            if filename.contains(&artist) {
                score += 45;
            }
            if filename.contains(&album) {
                score += 20;
            }
            if filename.contains(&title) {
                score += 60;
            }

            if let Some(len) = file.length
                && let Some(target_secs) = ctx.duration_secs
            {
                let diff = (len as i32 - target_secs as i32).abs();
                if diff <= 2 {
                    score += 20;
                } else if diff <= 5 {
                    score += 10;
                } else if diff <= 15 {
                    score += 4;
                } else {
                    score -= 10;
                }
            }

            let ext = file
                .extension
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            match quality {
                Quality::HiRes | Quality::Lossless => {
                    if ext == "flac" {
                        score += 30;
                    } else if ext == "m4a" || ext == "alac" {
                        score += 6;
                    } else {
                        score -= 12;
                    }
                }
                Quality::High | Quality::Low => {
                    if ext == "mp3" || ext == "ogg" || ext == "aac" {
                        score += 6;
                    } else {
                        score -= 12;
                    }
                }
            }

            if let Some(bitrate) = file.bit_rate {
                if bitrate >= 900 {
                    score += 10;
                } else if bitrate >= 320 {
                    score += 4;
                }
            }

            let candidate = SoulSeekCandidate {
                username: resp.username.clone(),
                filename: file.filename.clone(),
                size: file.size,
                score,
            };

            if best.as_ref().is_none_or(|b| candidate.score > b.score) {
                best = Some(candidate);
            }
        }
    }

    best
}

#[derive(Debug, Clone)]
struct AlbumBundleFile {
    username: String,
    filename: String,
    size: i64,
    extension: String,
    normalized_filename: String,
    track_number: Option<u32>,
}

fn pick_track_from_complete_album_bundle(
    responses: &[SlskdSearchResponse],
    ctx: &DownloadTrackContext,
    quality: &Quality,
) -> Option<SoulSeekCandidate> {
    let expected_tracks = ctx.album_track_count?;
    if expected_tracks == 0 {
        return None;
    }

    let mut bundles: HashMap<(String, String), Vec<AlbumBundleFile>> = HashMap::new();

    for resp in responses {
        for file in &resp.files {
            let Some(extension) = detect_audio_extension(file) else {
                continue;
            };
            let parent = normalized_parent_dir(&file.filename);
            if parent.is_empty() {
                continue;
            }

            bundles
                .entry((resp.username.clone(), parent))
                .or_default()
                .push(AlbumBundleFile {
                    username: resp.username.clone(),
                    filename: file.filename.clone(),
                    size: file.size,
                    extension,
                    normalized_filename: normalize(&file.filename),
                    track_number: parse_track_number_from_filename(&file.filename),
                });
        }
    }

    let artist = normalize(&ctx.artist_name);
    let album = normalize(&ctx.album_title);

    let mut best_bundle: Option<((String, String), Vec<AlbumBundleFile>, i32)> = None;
    for (key, files) in bundles {
        let unique_track_numbers: HashSet<u32> =
            files.iter().filter_map(|f| f.track_number).collect();
        let inferred_tracks = if unique_track_numbers.is_empty() {
            files.len()
        } else {
            unique_track_numbers.len()
        };
        if inferred_tracks < expected_tracks {
            continue;
        }

        let parent_norm = normalize(&key.1);
        let mut score = 0i32;
        if !artist.is_empty() && parent_norm.contains(&artist) {
            score += 35;
        }
        if !album.is_empty() && parent_norm.contains(&album) {
            score += 50;
        }

        let flac_count = files.iter().filter(|f| f.extension == "flac").count() as i32;
        score += flac_count * 2;
        score -= (inferred_tracks as i32 - expected_tracks as i32).abs();

        if best_bundle
            .as_ref()
            .is_none_or(|(_, _, best_score)| score > *best_score)
        {
            best_bundle = Some((key, files, score));
        }
    }

    let (_, files, _) = best_bundle?;
    let chosen = choose_track_from_bundle(&files, ctx, quality)?;

    Some(SoulSeekCandidate {
        username: chosen.username.clone(),
        filename: chosen.filename.clone(),
        size: chosen.size,
        score: 10_000,
    })
}

fn choose_track_from_bundle<'a>(
    files: &'a [AlbumBundleFile],
    ctx: &DownloadTrackContext,
    quality: &Quality,
) -> Option<&'a AlbumBundleFile> {
    let by_quality = |f: &&AlbumBundleFile| extension_quality_score(&f.extension, quality);

    if let Some(track_number) = ctx.track_number {
        let mut numbered_matches: Vec<&AlbumBundleFile> = files
            .iter()
            .filter(|f| f.track_number == Some(track_number))
            .collect();
        numbered_matches.sort_by_key(by_quality);
        numbered_matches.reverse();
        if let Some(best) = numbered_matches.first() {
            return Some(best);
        }
    }

    let title = normalize(&ctx.track_title);
    if !title.is_empty() {
        let mut title_matches: Vec<&AlbumBundleFile> = files
            .iter()
            .filter(|f| f.normalized_filename.contains(&title))
            .collect();
        title_matches.sort_by_key(by_quality);
        title_matches.reverse();
        if let Some(best) = title_matches.first() {
            // TODO: add fuzzy title matching within selected album bundle.
            return Some(best);
        }
    }

    let mut all_files: Vec<&AlbumBundleFile> = files.iter().collect();
    all_files.sort_by_key(by_quality);
    all_files.reverse();
    all_files.first().copied()
}

fn extension_quality_score(ext: &str, quality: &Quality) -> i32 {
    match quality {
        Quality::HiRes | Quality::Lossless => match ext {
            "flac" => 100,
            "m4a" | "alac" => 60,
            "wav" => 40,
            "aac" | "ogg" | "mp3" => 10,
            _ => 0,
        },
        Quality::High | Quality::Low => match ext {
            "mp3" | "ogg" | "aac" => 60,
            "flac" => 30,
            "m4a" | "alac" => 20,
            "wav" => 10,
            _ => 0,
        },
    }
}

fn detect_audio_extension(file: &SlskdSearchFile) -> Option<String> {
    let ext = file
        .extension
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| {
            Path::new(&file.filename)
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
        })?;

    if is_audio_extension(&ext) {
        Some(ext)
    } else {
        None
    }
}

fn is_audio_extension(ext: &str) -> bool {
    matches!(ext, "flac" | "m4a" | "alac" | "mp3" | "ogg" | "wav" | "aac")
}

fn normalized_parent_dir(filename: &str) -> String {
    let normalized = filename.replace('\\', "/");
    Path::new(&normalized)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn parse_track_number_from_filename(filename: &str) -> Option<u32> {
    let normalized = filename.replace('\\', "/");
    let stem = Path::new(&normalized).file_stem()?.to_str()?;
    let digits = stem
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

fn normalize(input: &str) -> String {
    input
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn percent_encode_path(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

fn is_rate_limited_error(err: &ProviderError) -> bool {
    err.0.contains("429") || err.0.to_ascii_lowercase().contains("too many requests")
}

fn sanitize_relative_path(input: &str) -> PathBuf {
    let relative = input.replace('\\', "/").trim_start_matches('/').to_string();
    let path = Path::new(&relative);
    let mut out = PathBuf::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out
}

fn transfer_state_text(t: &SlskdTransfer) -> String {
    t.state
        .as_deref()
        .or(t.state_description.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_transfer_failure(t: &SlskdTransfer) -> bool {
    let s = transfer_state_text(t);
    s.contains("rejected")
        || s.contains("failed")
        || s.contains("cancel")
        || s.contains("aborted")
        || s.contains("timed out")
        || s.contains("timeout")
        || s.contains("errored")
        || s.contains("denied")
}

fn is_transfer_complete_success(t: &SlskdTransfer) -> bool {
    if is_transfer_failure(t) {
        return false;
    }

    let s = transfer_state_text(t);
    if s.contains("completed") || s.contains("complete") || s.contains("succeeded") {
        return true;
    }

    if let (Some(total), Some(done), Some(remaining)) =
        (t.size, t.bytes_transferred, t.bytes_remaining)
    {
        return total > 0 && remaining <= 0 && done >= total;
    }

    false
}

#[derive(Debug, Serialize)]
struct SlskdLoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct SlskdTokenResponse {
    token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SlskdSearchRequest {
    id: Option<String>,
    search_text: String,
    search_timeout: Option<u32>,
    response_limit: Option<u32>,
    file_limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlskdSearch {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlskdSearchStatus {
    #[serde(default)]
    is_complete: bool,
    #[serde(default)]
    response_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlskdSearchResponse {
    username: String,
    files: Vec<SlskdSearchFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlskdSearchFile {
    filename: String,
    size: i64,
    #[serde(default)]
    length: Option<u32>,
    #[serde(default)]
    bit_rate: Option<u32>,
    #[serde(default)]
    extension: Option<String>,
}

#[derive(Debug, Serialize)]
struct SlskdQueueDownloadRequest {
    filename: String,
    size: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlskdTransferUserResponse {
    directories: Vec<SlskdTransferDirectory>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlskdTransferDirectory {
    #[serde(default)]
    directory: Option<String>,
    files: Vec<SlskdTransfer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlskdTransfer {
    filename: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    state_description: Option<String>,
    #[serde(default)]
    exception: Option<String>,
    #[serde(default)]
    size: Option<i64>,
    #[serde(default)]
    bytes_remaining: Option<i64>,
    #[serde(default)]
    bytes_transferred: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context(
        track_title: &str,
        track_number: u32,
        album_track_count: usize,
    ) -> DownloadTrackContext {
        DownloadTrackContext {
            artist_name: "The Artist".to_string(),
            album_title: "The Album".to_string(),
            track_title: track_title.to_string(),
            track_number: Some(track_number),
            album_track_count: Some(album_track_count),
            duration_secs: None,
        }
    }

    fn search_file(filename: &str, size: i64) -> SlskdSearchFile {
        SlskdSearchFile {
            filename: filename.to_string(),
            size,
            length: None,
            bit_rate: None,
            extension: Path::new(filename)
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_string()),
        }
    }

    fn transfer_with_state(state: &str) -> SlskdTransfer {
        SlskdTransfer {
            filename: "track.flac".to_string(),
            state: Some(state.to_string()),
            state_description: Some(state.to_string()),
            exception: None,
            size: Some(100),
            bytes_remaining: Some(0),
            bytes_transferred: Some(100),
        }
    }

    #[test]
    fn transfer_failure_detects_rejected_terminal_state() {
        let t = transfer_with_state("Completed, Rejected");
        assert!(is_transfer_failure(&t));
        assert!(!is_transfer_complete_success(&t));
    }

    #[test]
    fn transfer_success_detects_completed_succeeded_state() {
        let t = transfer_with_state("Completed, Succeeded");
        assert!(!is_transfer_failure(&t));
        assert!(is_transfer_complete_success(&t));
    }

    #[test]
    fn transfer_success_detects_byte_completion_without_state_text() {
        let t = SlskdTransfer {
            filename: "track.flac".to_string(),
            state: Some("InProgress".to_string()),
            state_description: None,
            exception: None,
            size: Some(500),
            bytes_remaining: Some(0),
            bytes_transferred: Some(500),
        };
        assert!(is_transfer_complete_success(&t));
    }

    #[test]
    fn resolve_local_download_paths_includes_leaf_directory_variant() {
        let source = SoulSeekSource::new(
            reqwest::Client::new(),
            "http://127.0.0.1:5030".to_string(),
            "".to_string(),
            "".to_string(),
            "/tmp/slskd-downloads".to_string(),
        );

        let paths = source.resolve_local_download_paths(
            Some("audiophile\\ATMOS\\Frank Zappa\\Over-Nite Sensation"),
            "audiophile\\ATMOS\\Frank Zappa\\Over-Nite Sensation\\1-03 Dirty Love.m4a",
        );

        let expected_leaf =
            PathBuf::from("/tmp/slskd-downloads/Over-Nite Sensation/1-03 Dirty Love.m4a");

        assert!(paths.contains(&expected_leaf));
    }

    #[test]
    fn sanitize_relative_path_strips_parent_segments() {
        let cleaned = sanitize_relative_path("../../bad\\../music/track.flac");
        assert_eq!(cleaned, PathBuf::from("bad/music/track.flac"));
    }

    #[test]
    fn album_bundle_selection_requires_complete_track_count() {
        let ctx = test_context("Song Two", 2, 3);
        let responses = vec![SlskdSearchResponse {
            username: "user1".to_string(),
            files: vec![
                search_file("The Artist/The Album/01 - Song One.flac", 100),
                search_file("The Artist/The Album/02 - Song Two.flac", 100),
            ],
        }];

        let candidate = pick_track_from_complete_album_bundle(&responses, &ctx, &Quality::Lossless);
        assert!(candidate.is_none());
    }

    #[test]
    fn album_bundle_selection_picks_requested_track_from_complete_bundle() {
        let ctx = test_context("Song Two", 2, 2);
        let responses = vec![SlskdSearchResponse {
            username: "user1".to_string(),
            files: vec![
                search_file("The Artist/The Album/01 - Song One.flac", 100),
                search_file("The Artist/The Album/02 - Song Two.flac", 100),
            ],
        }];

        let candidate = pick_track_from_complete_album_bundle(&responses, &ctx, &Quality::Lossless)
            .expect("expected complete album candidate");
        assert_eq!(candidate.username, "user1");
        assert!(candidate.filename.contains("02 - Song Two"));
    }

    #[test]
    fn album_bundle_selection_prefers_track_number_over_title() {
        let ctx = test_context("Song One", 2, 2);
        let responses = vec![SlskdSearchResponse {
            username: "user1".to_string(),
            files: vec![
                search_file("The Artist/The Album/01 - Song One.flac", 100),
                search_file("The Artist/The Album/02 - Interlude.flac", 100),
            ],
        }];

        let candidate = pick_track_from_complete_album_bundle(&responses, &ctx, &Quality::Lossless)
            .expect("expected complete album candidate");
        assert!(candidate.filename.contains("02 - Interlude"));
    }
}
