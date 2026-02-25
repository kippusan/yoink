//! Types shared between server (binary crate) and client (WASM lib crate).
//!
//! These types are used in Leptos server function signatures, so they must be
//! available to both the SSR binary and the hydrated WASM client.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Core domain types ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Resolving,
    Downloading,
    Completed,
    Failed,
}

impl DownloadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Resolving => "resolving",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: u64,
    pub album_id: i64,
    pub artist_id: i64,
    pub album_title: String,
    pub status: DownloadStatus,
    pub quality: String,
    pub total_tracks: usize,
    pub completed_tracks: usize,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredArtist {
    pub id: i64,
    pub name: String,
    pub picture: Option<String>,
    pub tidal_url: Option<String>,
    pub quality_profile: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredAlbum {
    pub id: i64,
    pub artist_id: i64,
    pub title: String,
    pub album_type: Option<String>,
    pub release_date: Option<String>,
    pub cover: Option<String>,
    pub tidal_url: Option<String>,
    pub explicit: bool,
    pub monitored: bool,
    pub acquired: bool,
    pub wanted: bool,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: i64,
    pub title: String,
    pub track_number: u32,
    pub duration_secs: u32,
    pub duration_display: String,
}

// ── Data helpers (pure transforms) ──────────────────────────

/// Group albums by artist_id, sorted newest-first within each group.
pub fn build_albums_by_artist(albums: Vec<MonitoredAlbum>) -> HashMap<i64, Vec<MonitoredAlbum>> {
    let mut map: HashMap<i64, Vec<MonitoredAlbum>> = HashMap::new();
    for album in albums {
        map.entry(album.artist_id).or_default().push(album);
    }
    for albums in map.values_mut() {
        albums.sort_by(|a, b| {
            b.release_date
                .cmp(&a.release_date)
                .then_with(|| a.title.cmp(&b.title))
        });
    }
    map
}

/// For each album_id, keep only the most recently updated job.
pub fn build_latest_jobs(jobs: Vec<DownloadJob>) -> HashMap<i64, DownloadJob> {
    let mut map: HashMap<i64, DownloadJob> = HashMap::new();
    for job in jobs {
        map.entry(job.album_id)
            .and_modify(|existing| {
                if job.updated_at > existing.updated_at {
                    *existing = job.clone();
                }
            })
            .or_insert(job);
    }
    map
}

/// Map artist id → name for display.
pub fn build_artist_names(artists: &[MonitoredArtist]) -> HashMap<i64, String> {
    artists.iter().map(|a| (a.id, a.name.clone())).collect()
}

// ── Display helpers ─────────────────────────────────────────

pub fn status_label_text(status: &DownloadStatus, completed: usize, total: usize) -> String {
    match status {
        DownloadStatus::Queued => "Queued".to_string(),
        DownloadStatus::Resolving => "Resolving".to_string(),
        DownloadStatus::Downloading => {
            if total > 0 {
                format!("Downloading {completed}/{total}")
            } else {
                "Downloading".to_string()
            }
        }
        DownloadStatus::Completed => "Completed".to_string(),
        DownloadStatus::Failed => "Failed".to_string(),
    }
}

// ── Search result DTO (used by Artists page) ───────────────

/// A search result from the HiFi API, serializable for client use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchArtistResult {
    pub id: i64,
    pub name: String,
    pub picture: Option<String>,
    pub url: Option<String>,
}

// ── Asset/URL helpers ───────────────────────────────────────

pub fn tidal_image_url(image_id: &str, size: u16) -> String {
    format!("/api/image/{image_id}/{size}")
}

pub fn album_cover_url(album: &MonitoredAlbum, size: u16) -> Option<String> {
    album.cover.as_deref().map(|id| tidal_image_url(id, size))
}

pub fn album_profile_url(album: &MonitoredAlbum) -> Option<String> {
    album
        .tidal_url
        .as_deref()
        .map(|url| url.replace("http://", "https://"))
}

pub fn monitored_artist_image_url(artist: &MonitoredArtist, size: u16) -> Option<String> {
    artist
        .picture
        .as_deref()
        .map(|id| tidal_image_url(id, size))
}

pub fn search_artist_image_url(artist: &SearchArtistResult, size: u16) -> Option<String> {
    artist
        .picture
        .as_deref()
        .map(|id| tidal_image_url(id, size))
}

pub fn search_artist_profile_url(artist: &SearchArtistResult) -> String {
    artist
        .url
        .clone()
        .unwrap_or_else(|| format!("https://tidal.com/artist/{}", artist.id))
}

pub fn monitored_artist_profile_url(artist: &MonitoredArtist) -> String {
    artist
        .tidal_url
        .clone()
        .unwrap_or_else(|| format!("https://tidal.com/artist/{}", artist.id))
}

pub fn album_type_label(album_type: Option<&str>, title: &str) -> &'static str {
    if let Some(kind) = album_type {
        let k = kind.to_ascii_lowercase();
        if k.contains("ep") {
            return "EP";
        }
        if k.contains("single") {
            return "Single";
        }
        if k.contains("album") {
            return "Album";
        }
    }
    let t = title.to_ascii_lowercase();
    if t.contains(" ep") || t.ends_with("ep") || t.contains("(ep") {
        return "EP";
    }
    if t.contains(" single") || t.ends_with("single") || t.contains("(single") {
        return "Single";
    }
    "Album"
}

pub fn album_type_rank(album_type: Option<&str>, title: &str) -> u8 {
    match album_type_label(album_type, title) {
        "Album" => 0,
        "EP" => 1,
        "Single" => 2,
        _ => 3,
    }
}

pub fn status_class(status: &DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued | DownloadStatus::Resolving => "pill status-queued",
        DownloadStatus::Downloading => "pill status-downloading",
        DownloadStatus::Completed => "pill status-completed",
        DownloadStatus::Failed => "pill status-failed",
    }
}

// ── Server-side context for Leptos server functions ─────────

/// Holds the shared in-memory state that server functions need to read.
///
/// This is only compiled when the `ssr` feature is active. It is provided
/// via `leptos::context::provide_context` in main.rs and consumed via
/// `use_context::<ServerContext>()` inside `#[server]` functions.
#[cfg(feature = "ssr")]
type AsyncFnResult<T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send>>;

#[cfg(feature = "ssr")]
pub type SearchArtistsFn =
    std::sync::Arc<dyn Fn(String) -> AsyncFnResult<Vec<SearchArtistResult>> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type FetchTracksFn = std::sync::Arc<dyn Fn(i64) -> AsyncFnResult<Vec<TrackInfo>> + Send + Sync>;

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct ServerContext {
    pub monitored_artists: std::sync::Arc<tokio::sync::RwLock<Vec<MonitoredArtist>>>,
    pub monitored_albums: std::sync::Arc<tokio::sync::RwLock<Vec<MonitoredAlbum>>>,
    pub download_jobs: std::sync::Arc<tokio::sync::RwLock<Vec<DownloadJob>>>,
    pub search_artists: SearchArtistsFn,
    pub fetch_tracks: FetchTracksFn,
}
