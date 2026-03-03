use serde::{Deserialize, Serialize};

// Re-export shared types so the rest of the binary crate can keep using
// `crate::models::MonitoredAlbum` etc. without changes.
pub(crate) use yoink_shared::{
    DownloadJob, DownloadStatus, MonitoredAlbum, MonitoredArtist, TrackInfo,
};

// ── Server-only types (not needed in WASM client) ───────────

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SearchQuery {
    pub(crate) q: Option<String>,
}

/// Search result returned by /api/search?q=
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchResultArtist {
    pub(crate) provider: String,
    pub(crate) external_id: String,
    pub(crate) name: String,
    pub(crate) image_url: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) already_monitored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) disambiguation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) artist_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) country: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) popularity: Option<u8>,
}

/// Album search result returned by /api/search/albums?q=
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchResultAlbum {
    pub(crate) provider: String,
    pub(crate) external_id: String,
    pub(crate) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) album_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) release_date: Option<String>,
    pub(crate) cover_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
    pub(crate) explicit: bool,
    pub(crate) artist_name: String,
    pub(crate) artist_external_id: String,
}

/// Track search result returned by /api/search/tracks?q=
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchResultTrack {
    pub(crate) provider: String,
    pub(crate) external_id: String,
    pub(crate) title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) duration_secs: u32,
    pub(crate) duration_display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) isrc: Option<String>,
    pub(crate) explicit: bool,
    pub(crate) artist_name: String,
    pub(crate) artist_external_id: String,
    pub(crate) album_title: String,
    pub(crate) album_external_id: String,
    pub(crate) album_cover_url: Option<String>,
}
