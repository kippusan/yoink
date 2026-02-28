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
    pub id: String,       // UUID v7
    pub album_id: String, // UUID v7
    pub source: String,   // download source: "tidal", "soulseek"
    pub album_title: String,
    pub artist_name: String, // denormalized for display
    pub status: DownloadStatus,
    pub quality: String, // "lossless", "hires", "lossy"
    pub total_tracks: usize,
    pub completed_tracks: usize,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredArtist {
    pub id: String, // UUID v7
    pub name: String,
    pub image_url: Option<String>, // resolved URL
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitoredAlbum {
    pub id: String,        // UUID v7
    pub artist_id: String, // UUID v7
    pub title: String,
    pub album_type: Option<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>, // resolved URL
    pub explicit: bool,
    pub monitored: bool,
    pub acquired: bool,
    pub wanted: bool,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: String, // UUID v7
    pub title: String,
    pub version: Option<String>,
    pub disc_number: u32,
    pub track_number: u32,
    pub duration_secs: u32,
    pub duration_display: String,
    pub isrc: Option<String>,
    pub explicit: bool,
}

/// Provider link info for the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderLink {
    pub provider: String, // "tidal", "musicbrainz", "deezer"
    pub external_id: String,
    pub external_url: Option<String>,
    pub external_name: Option<String>,
}

/// Potential cross-provider match suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSuggestion {
    pub id: String,
    pub scope_type: String,
    pub scope_id: String,
    pub left_provider: String,
    pub left_external_id: String,
    pub right_provider: String,
    pub right_external_id: String,
    pub match_kind: String,
    pub confidence: u8,
    pub explanation: Option<String>,
    pub external_name: Option<String>,
    pub external_url: Option<String>,
    pub image_url: Option<String>,
    pub disambiguation: Option<String>,
    pub artist_type: Option<String>,
    pub country: Option<String>,
    pub tags: Vec<String>,
    pub popularity: Option<u8>,
    pub status: String,
}

// ── Data helpers (pure transforms) ──────────────────────────

/// Group albums by artist_id, sorted newest-first within each group.
pub fn build_albums_by_artist(albums: Vec<MonitoredAlbum>) -> HashMap<String, Vec<MonitoredAlbum>> {
    let mut map: HashMap<String, Vec<MonitoredAlbum>> = HashMap::new();
    for album in albums {
        map.entry(album.artist_id.clone()).or_default().push(album);
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
pub fn build_latest_jobs(jobs: Vec<DownloadJob>) -> HashMap<String, DownloadJob> {
    let mut map: HashMap<String, DownloadJob> = HashMap::new();
    for job in jobs {
        map.entry(job.album_id.clone())
            .and_modify(|existing| {
                if job.updated_at > existing.updated_at {
                    *existing = job.clone();
                }
            })
            .or_insert(job);
    }
    map
}

/// Map artist id -> name for display.
pub fn build_artist_names(artists: &[MonitoredArtist]) -> HashMap<String, String> {
    artists
        .iter()
        .map(|a| (a.id.clone(), a.name.clone()))
        .collect()
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

/// A search result from a metadata provider, serializable for client use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchArtistResult {
    pub provider: String,    // which provider returned this
    pub external_id: String, // provider's ID as string
    pub name: String,
    pub image_url: Option<String>, // resolved image URL
    pub url: Option<String>,
    /// Short disambiguation comment (e.g. "British electronic duo").
    pub disambiguation: Option<String>,
    /// Artist type: "Person", "Group", "Orchestra", etc.
    pub artist_type: Option<String>,
    /// Country or area name.
    pub country: Option<String>,
    /// Genre/tag names, most relevant first (top 3–5).
    pub tags: Vec<String>,
    /// Popularity percentage (0–100), if available.
    pub popularity: Option<u8>,
}

// ── Asset/URL helpers ───────────────────────────────────────

/// Build an image proxy URL for a given provider and image reference.
pub fn provider_image_url(provider: &str, image_ref: &str, size: u16) -> String {
    format!("/api/image/{provider}/{image_ref}/{size}")
}

/// Get the cover URL for an album (already a full URL or None).
pub fn album_cover_url(album: &MonitoredAlbum, _size: u16) -> Option<String> {
    album.cover_url.clone()
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

/// Human-readable display name for a provider ID (e.g. "tidal" -> "Tidal").
pub fn provider_display_name(provider: &str) -> String {
    match provider {
        "tidal" => "Tidal".to_string(),
        "musicbrainz" => "MusicBrainz".to_string(),
        "deezer" => "Deezer".to_string(),
        "soulseek" => "SoulSeek".to_string(),
        other => {
            let mut c = other.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        }
    }
}

pub fn status_class(status: &DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued => "pill status-queued",
        DownloadStatus::Resolving => "pill status-resolving",
        DownloadStatus::Downloading => "pill status-downloading",
        DownloadStatus::Completed => "pill status-completed",
        DownloadStatus::Failed => "pill status-failed",
    }
}

// ── Actions (shared between server and WASM client) ─────────

/// A user-initiated action that the server can execute.
///
/// Serialized by the WASM client, sent to the `dispatch_action` server function,
/// and executed on the server where `AppState` is available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerAction {
    ToggleAlbumMonitor {
        album_id: String,
        monitored: bool,
    },
    BulkMonitor {
        artist_id: String,
        monitored: bool,
    },
    SyncArtistAlbums {
        artist_id: String,
    },
    RemoveArtist {
        artist_id: String,
        remove_files: bool,
    },
    AddArtist {
        name: String,
        provider: String,    // which provider this came from
        external_id: String, // provider's ID
        image_url: Option<String>,
        external_url: Option<String>,
    },
    LinkArtistProvider {
        artist_id: String,
        provider: String,
        external_id: String,
        external_url: Option<String>,
        external_name: Option<String>,
        image_ref: Option<String>,
    },
    UnlinkArtistProvider {
        artist_id: String,
        provider: String,
        external_id: String,
    },
    CancelDownload {
        job_id: String,
    },
    ClearCompleted,
    RetryDownload {
        album_id: String,
    },
    RemoveAlbumFiles {
        album_id: String,
        unmonitor: bool,
    },
    AcceptMatchSuggestion {
        suggestion_id: String,
    },
    DismissMatchSuggestion {
        suggestion_id: String,
    },
    RefreshMatchSuggestions {
        artist_id: String,
    },
    MergeAlbums {
        target_album_id: String,
        source_album_id: String,
        /// If provided, override the surviving album's title.
        result_title: Option<String>,
        /// If provided, override the surviving album's cover URL.
        result_cover_url: Option<String>,
    },
    RetagLibrary,
    ScanImportLibrary,
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
pub type SearchArtistsScopedFn =
    std::sync::Arc<dyn Fn(String, String) -> AsyncFnResult<Vec<SearchArtistResult>> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type ListProvidersFn = std::sync::Arc<dyn Fn() -> Vec<String> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type FetchTracksFn =
    std::sync::Arc<dyn Fn(String) -> AsyncFnResult<Vec<TrackInfo>> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type FetchArtistLinksFn =
    std::sync::Arc<dyn Fn(String) -> AsyncFnResult<Vec<ProviderLink>> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type FetchAlbumLinksFn =
    std::sync::Arc<dyn Fn(String) -> AsyncFnResult<Vec<ProviderLink>> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type FetchArtistMatchSuggestionsFn =
    std::sync::Arc<dyn Fn(String) -> AsyncFnResult<Vec<MatchSuggestion>> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type FetchAlbumMatchSuggestionsFn =
    std::sync::Arc<dyn Fn(String) -> AsyncFnResult<Vec<MatchSuggestion>> + Send + Sync>;

#[cfg(feature = "ssr")]
pub type DispatchActionFn = std::sync::Arc<dyn Fn(ServerAction) -> AsyncFnResult<()> + Send + Sync>;

#[cfg(feature = "ssr")]
#[derive(Clone)]
pub struct ServerContext {
    pub monitored_artists: std::sync::Arc<tokio::sync::RwLock<Vec<MonitoredArtist>>>,
    pub monitored_albums: std::sync::Arc<tokio::sync::RwLock<Vec<MonitoredAlbum>>>,
    pub download_jobs: std::sync::Arc<tokio::sync::RwLock<Vec<DownloadJob>>>,
    pub search_artists: SearchArtistsFn,
    pub search_artists_scoped: SearchArtistsScopedFn,
    pub list_providers: ListProvidersFn,
    pub fetch_tracks: FetchTracksFn,
    pub fetch_artist_links: FetchArtistLinksFn,
    pub fetch_album_links: FetchAlbumLinksFn,
    pub fetch_artist_match_suggestions: FetchArtistMatchSuggestionsFn,
    pub fetch_album_match_suggestions: FetchAlbumMatchSuggestionsFn,
    pub dispatch_action: DispatchActionFn,
}
