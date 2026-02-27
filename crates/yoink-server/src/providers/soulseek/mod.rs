use std::{path::{Path, PathBuf}, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::{DownloadSource, DownloadTrackContext, PlaybackInfo, ProviderError, Quality};

pub(crate) struct SoulSeekSource {
    http: reqwest::Client,
    slskd_base_url: String,
    username: String,
    password: String,
    downloads_dir: PathBuf,
    token: RwLock<Option<String>>,
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
        let mut req = self.http.post(url).json(body).timeout(Duration::from_secs(30));
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
        let req = SlskdSearchRequest {
            id: None,
            search_text: query.to_string(),
            search_timeout: Some(12),
            response_limit: Some(100),
            file_limit: Some(500),
        };
        self.post_json("/api/v0/searches", &req).await
    }

    async fn poll_search_responses(
        &self,
        search_id: &str,
        wait_secs: u64,
    ) -> Result<Vec<SlskdSearchResponse>, ProviderError> {
        let mut elapsed = 0u64;
        let path = format!("/api/v0/searches/{search_id}/responses");

        while elapsed < wait_secs {
            let responses: Vec<SlskdSearchResponse> = self.get_json(&path).await?;
            if !responses.is_empty() {
                return Ok(responses);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            elapsed += 2;
        }

        Ok(Vec::new())
    }

    async fn enqueue_download(&self, username: &str, filename: &str, size: i64) -> Result<(), ProviderError> {
        let path = format!("/api/v0/transfers/downloads/{}", percent_encode_path(username));
        let body = vec![SlskdQueueDownloadRequest {
            filename: filename.to_string(),
            size,
        }];

        let token = self.auth_token().await?;
        let url = format!("{}{}", self.slskd_base_url, path);
        let mut req = self.http.post(url).json(&body).timeout(Duration::from_secs(30));
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
        let path = format!("/api/v0/transfers/downloads/{}", percent_encode_path(username));
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
                debug!(username, filename, "soulseek transfer found, waiting for completion");
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

        let query = format!("{} {} {}", ctx.artist_name, ctx.album_title, ctx.track_title);
        let search = self.start_search(&query).await?;

        let responses = self.poll_search_responses(&search.id, 18).await?;
        if responses.is_empty() {
            return Err(ProviderError(format!(
                "No SoulSeek search responses for track '{}'",
                ctx.track_title
            )));
        }

        let candidate = pick_best_candidate(&responses, ctx, quality).ok_or_else(|| {
            ProviderError(format!(
                "No suitable SoulSeek candidate for '{}'",
                ctx.track_title
            ))
        })?;

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

    if let (Some(total), Some(done), Some(remaining)) = (t.size, t.bytes_transferred, t.bytes_remaining)
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

        let expected_leaf = PathBuf::from(
            "/tmp/slskd-downloads/Over-Nite Sensation/1-03 Dirty Love.m4a",
        );

        assert!(paths.contains(&expected_leaf));
    }

    #[test]
    fn sanitize_relative_path_strips_parent_segments() {
        let cleaned = sanitize_relative_path("../../bad\\../music/track.flac");
        assert_eq!(cleaned, PathBuf::from("bad/music/track.flac"));
    }
}
