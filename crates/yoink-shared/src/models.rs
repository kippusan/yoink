use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    pub id: Uuid,
    pub album_id: Uuid,
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
    pub id: Uuid,
    pub name: String,
    pub image_url: Option<String>,
    pub bio: Option<String>,
    pub added_at: DateTime<Utc>,
}

/// A raw artist credit from a provider, stored on the album.
/// Used to display all album artists even when some aren't monitored locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistCredit {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitoredAlbum {
    pub id: Uuid,
    /// Primary (first) artist — kept for backward compatibility and as a
    /// convenient shorthand for the common single-artist case.
    pub artist_id: Uuid,
    /// All artists associated with this album, ordered by display priority.
    /// The first entry always equals `artist_id`.
    #[serde(default)]
    pub artist_ids: Vec<Uuid>,
    /// Raw artist credits from providers. Includes artists that may not be
    /// monitored locally. Used for display on the album detail page.
    #[serde(default)]
    pub artist_credits: Vec<ArtistCredit>,
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
    pub id: Uuid,
    pub title: String,
    pub version: Option<String>,
    pub disc_number: u32,
    pub track_number: u32,
    pub duration_secs: u32,
    pub duration_display: String,
    pub isrc: Option<String>,
    pub explicit: bool,
    /// Track-level artist string (may differ from album artist for features/collabs).
    pub track_artist: Option<String>,
    /// Local file path relative to the music root (populated for acquired albums).
    pub file_path: Option<String>,
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
    pub id: Uuid,
    pub scope_type: String,
    pub scope_id: Uuid,
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

/// An artist image option from a linked provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistImageOption {
    pub provider: String,
    pub image_url: String,
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
