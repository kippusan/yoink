use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Re-export shared types so the rest of the binary crate can keep using
// `crate::models::MonitoredAlbum` etc. without changes.
pub(crate) use yoink::shared::{DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist};

// ── Server-only types (not needed in WASM client) ───────────

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SearchQuery {
    pub(crate) q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiResponse {
    pub(crate) data: SearchData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiArtistAlbumsResponse {
    pub(crate) albums: HifiAlbumPage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiAlbumResponse {
    pub(crate) data: HifiAlbumData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiAlbumData {
    pub(crate) items: Vec<HifiAlbumItem>,
    #[serde(flatten)]
    pub(crate) extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum HifiAlbumItem {
    Item { item: HifiTrack },
    Track(HifiTrack),
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiAlbumPage {
    pub(crate) items: Vec<HifiAlbum>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HifiAlbum {
    pub(crate) id: i64,
    pub(crate) title: String,
    #[serde(rename = "type")]
    pub(crate) album_type: Option<String>,
    #[serde(rename = "releaseDate")]
    pub(crate) release_date: Option<String>,
    pub(crate) cover: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) explicit: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HifiTrack {
    pub(crate) id: i64,
    pub(crate) title: String,
    #[serde(rename = "trackNumber")]
    pub(crate) track_number: Option<u32>,
    pub(crate) duration: Option<u32>,
    #[serde(flatten)]
    pub(crate) extra: HashMap<String, serde_json::Value>,
}

/// Track info returned by /api/albums/{id}/tracks
#[derive(Debug, Clone, Serialize)]
pub(crate) struct TrackInfo {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) track_number: u32,
    pub(crate) duration_secs: u32,
    pub(crate) duration_display: String,
}

/// Search result returned by /api/search?q=
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchResultArtist {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) picture_url: Option<String>,
    pub(crate) tidal_url: String,
    pub(crate) already_monitored: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiPlaybackResponse {
    pub(crate) data: HifiPlaybackData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiPlaybackData {
    #[serde(rename = "manifestMimeType")]
    pub(crate) manifest_mime_type: String,
    pub(crate) manifest: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BtsManifest {
    pub(crate) urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchData {
    pub(crate) artists: Option<PagedArtists>,
    pub(crate) items: Option<Vec<HifiArtist>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PagedArtists {
    pub(crate) items: Vec<HifiArtist>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HifiArtist {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) picture: Option<String>,
    #[serde(rename = "selectedAlbumCoverFallback")]
    pub(crate) selected_album_cover_fallback: Option<String>,
    pub(crate) url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FeedInstance {
    pub(crate) url: String,
    pub(crate) version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DownInstance {
    pub(crate) url: String,
    pub(crate) status: Option<u16>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RankedInstance {
    pub(crate) url: String,
    pub(crate) version: String,
    pub(crate) source: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UptimeFeed {
    pub(crate) api: Vec<FeedInstance>,
    pub(crate) streaming: Vec<FeedInstance>,
    pub(crate) down: Vec<DownInstance>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InstancesResponse {
    pub(crate) manual_override: Option<String>,
    pub(crate) active_base_url: Option<String>,
    pub(crate) last_refresh: Option<DateTime<Utc>>,
    pub(crate) ranked: Vec<RankedInstance>,
    pub(crate) api: Vec<FeedInstance>,
    pub(crate) streaming: Vec<FeedInstance>,
    pub(crate) down: Vec<DownInstance>,
}
