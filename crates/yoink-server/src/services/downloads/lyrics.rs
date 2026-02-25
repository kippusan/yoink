use std::path::Path;
use std::time::Duration;

use lrclib_api_rs::{LRCLibAPI, types::GetLyricsResponse};
use tokio::fs;

use crate::state::AppState;

pub(crate) struct LyricsBundle {
    pub(crate) embedded_text: Option<String>,
    pub(crate) synced_lrc: Option<String>,
}

pub(crate) async fn fetch_track_lyrics(
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

pub(crate) async fn write_lrc_sidecar(audio_path: &Path, synced_lrc: &str) -> Result<(), String> {
    let sidecar_path = audio_path.with_extension("lrc");
    fs::write(&sidecar_path, synced_lrc)
        .await
        .map_err(|err| format!("failed writing sidecar {}: {err}", sidecar_path.display()))
}
