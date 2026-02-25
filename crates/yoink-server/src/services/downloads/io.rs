use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::config::DOWNLOAD_CHUNK_SIZE;

use super::manifest::DownloadPayload;

pub(crate) async fn download_payload_to_file(
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

pub(crate) async fn has_flac_stream_marker(path: &PathBuf) -> Result<bool, String> {
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

pub(crate) async fn sniff_media_container(path: &PathBuf) -> Result<String, String> {
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

pub(crate) fn normalize_quality(raw: &str) -> String {
    let upper = raw.trim().to_ascii_uppercase();
    match upper.as_str() {
        "HI_RES_LOSSLESS" | "HI_RES_LOSLESS" | "HIRES_LOSSLESS" | "HIRES" => {
            "HI_RES_LOSSLESS".to_string()
        }
        "LOSSLESS" | "HIGH" | "LOW" => upper,
        _ => {
            tracing::warn!(quality = %raw, normalized = "LOSSLESS", "Unknown quality requested, using LOSSLESS");
            "LOSSLESS".to_string()
        }
    }
}

pub(crate) fn parse_track_number_from_path(path: &Path) -> Option<u32> {
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

pub(crate) fn extract_year(release_date: &str) -> String {
    let year = release_date.chars().take(4).collect::<String>();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        year
    } else {
        String::new()
    }
}
