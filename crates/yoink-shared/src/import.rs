use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Match quality for a discovered local album during import preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMatchStatus {
    /// Exact match found (artist + album title + year all agree).
    Matched,
    /// Artist matched but album only partially matched (fuzzy title or missing year).
    Partial,
    /// No match found in any provider.
    Unmatched,
}

impl ImportMatchStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Partial => "partial",
            Self::Unmatched => "unmatched",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            Self::Matched => "pill status-completed",
            Self::Partial => "pill status-resolving",
            Self::Unmatched => "pill status-failed",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Matched => "Matched",
            Self::Partial => "Partial Match",
            Self::Unmatched => "Unmatched",
        }
    }
}

/// A candidate album match for a discovered local folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAlbumCandidate {
    pub album_id: Option<Uuid>,
    pub artist_id: Uuid,
    pub artist_name: String,
    pub album_title: String,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,
    pub album_type: Option<String>,
    pub explicit: bool,
    pub monitored: bool,
    pub acquired: bool,
    pub confidence: u8,
}

/// A discovered local album directory with match candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreviewItem {
    pub id: String,
    pub relative_path: String,
    pub discovered_artist: String,
    pub discovered_album: String,
    pub discovered_year: Option<String>,
    pub match_status: ImportMatchStatus,
    pub candidates: Vec<ImportAlbumCandidate>,
    pub selected_candidate: Option<usize>,
    pub already_imported: bool,
    pub audio_file_count: usize,
}

/// User-confirmed import selection for a single item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportConfirmation {
    pub preview_id: String,
    pub artist_name: String,
    pub album_title: String,
    pub year: Option<String>,
    pub artist_id: Option<Uuid>,
    pub album_id: Option<Uuid>,
}

/// Summary of a confirmed import run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResultSummary {
    pub total_selected: usize,
    pub imported: usize,
    pub artists_added: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}
