use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DownloadStatus {
    Queued,
    Resolving,
    Downloading,
    Completed,
    Failed,
}

impl DownloadStatus {
    pub(crate) fn as_str(&self) -> &'static str {
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
pub(crate) struct DownloadJob {
    pub(crate) id: u64,
    pub(crate) album_id: i64,
    pub(crate) artist_id: i64,
    pub(crate) album_title: String,
    pub(crate) status: DownloadStatus,
    pub(crate) quality: String,
    pub(crate) total_tracks: usize,
    pub(crate) completed_tracks: usize,
    pub(crate) error: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MonitoredArtist {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) picture: Option<String>,
    pub(crate) tidal_url: Option<String>,
    pub(crate) quality_profile: String,
    pub(crate) added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MonitoredAlbum {
    pub(crate) id: i64,
    pub(crate) artist_id: i64,
    pub(crate) title: String,
    pub(crate) album_type: Option<String>,
    pub(crate) release_date: Option<String>,
    pub(crate) cover: Option<String>,
    pub(crate) tidal_url: Option<String>,
    pub(crate) explicit: bool,
    pub(crate) monitored: bool,
    pub(crate) acquired: bool,
    pub(crate) wanted: bool,
    pub(crate) added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SearchQuery {
    pub(crate) q: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AddArtistForm {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) picture: Option<String>,
    pub(crate) tidal_url: Option<String>,
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SyncArtistAlbumsForm {
    pub(crate) artist_id: i64,
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ToggleAlbumMonitorForm {
    pub(crate) album_id: i64,
    pub(crate) monitored: bool,
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RetryDownloadForm {
    pub(crate) album_id: i64,
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RemoveArtistForm {
    pub(crate) artist_id: i64,
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BulkMonitorForm {
    pub(crate) artist_id: i64,
    pub(crate) monitored: bool,
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CancelDownloadForm {
    pub(crate) job_id: u64,
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ClearCompletedForm {
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RetagLibraryForm {
    pub(crate) return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ScanImportLibraryForm {
    pub(crate) return_to: Option<String>,
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
