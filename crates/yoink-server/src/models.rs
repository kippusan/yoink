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
}
