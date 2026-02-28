use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    pub id: String,
    pub album_id: String,
    pub source: String,
    pub album_title: String,
    pub artist_name: String,
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
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub bio: Option<String>,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitoredAlbum {
    pub id: String,
    pub artist_id: String,
    pub title: String,
    pub album_type: Option<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    pub explicit: bool,
    pub monitored: bool,
    pub acquired: bool,
    pub wanted: bool,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: String,
    pub title: String,
    pub version: Option<String>,
    pub disc_number: u32,
    pub track_number: u32,
    pub duration_secs: u32,
    pub duration_display: String,
    pub isrc: Option<String>,
    pub explicit: bool,
}

/// Provider link info for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderLink {
    pub provider: String,
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

/// A search result from a metadata provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchArtistResult {
    pub provider: String,
    pub external_id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub url: Option<String>,
    pub disambiguation: Option<String>,
    pub artist_type: Option<String>,
    pub country: Option<String>,
    pub tags: Vec<String>,
    pub popularity: Option<u8>,
}
