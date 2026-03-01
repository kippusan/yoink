pub(crate) mod deezer;
pub(crate) mod musicbrainz;
pub(crate) mod registry;
pub(crate) mod soulseek;
pub(crate) mod tidal;

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

// ── Shared helpers ──────────────────────────────────────────────────

/// Extract a display-ready artist string from a provider extra map.
/// Looks for "artists" or "artist" keys containing arrays of objects with "name".
pub(crate) fn extract_artist_display(extra: &HashMap<String, Value>) -> Option<String> {
    for key in ["artists", "artist"] {
        match extra.get(key) {
            Some(Value::Array(items)) if !items.is_empty() => {
                let names: Vec<&str> = items
                    .iter()
                    .filter_map(|v| match v {
                        Value::String(s) => Some(s.as_str()),
                        Value::Object(obj) => obj
                            .get("name")
                            .or_else(|| obj.get("title"))
                            .and_then(|n| n.as_str()),
                        _ => None,
                    })
                    .collect();
                if !names.is_empty() {
                    return Some(names.join("; "));
                }
            }
            Some(Value::String(s)) if !s.is_empty() => return Some(s.clone()),
            _ => {}
        }
    }
    None
}

// ── Provider error ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ProviderError(pub String);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ProviderError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ProviderError {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ── Shared provider types ───────────────────────────────────────────

/// An artist returned by a metadata provider search.
#[derive(Debug, Clone)]
pub(crate) struct ProviderArtist {
    pub external_id: String,
    pub name: String,
    pub image_ref: Option<String>,
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

/// A minimal artist reference attached to a provider album.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ProviderAlbumArtist {
    pub external_id: String,
    pub name: String,
}

/// An album returned by a metadata provider.
#[derive(Debug, Clone)]
pub(crate) struct ProviderAlbum {
    pub external_id: String,
    pub title: String,
    pub album_type: Option<String>,
    pub release_date: Option<String>,
    pub cover_ref: Option<String>,
    pub url: Option<String>,
    pub explicit: bool,
    /// Artists credited on this album (may be empty if provider doesn't supply them).
    pub artists: Vec<ProviderAlbumArtist>,
}

/// A track returned by a metadata provider.
#[derive(Debug, Clone)]
pub(crate) struct ProviderTrack {
    pub external_id: String,
    pub title: String,
    pub version: Option<String>,
    pub track_number: u32,
    pub disc_number: Option<u32>,
    pub duration_secs: u32,
    pub isrc: Option<String>,
    /// Display-ready track artist string (e.g. "Artist A feat. Artist B").
    pub artists: Option<String>,
    /// Whether the track is marked explicit.
    pub explicit: bool,
    /// Provider-specific extra metadata (for tagging).
    pub extra: HashMap<String, Value>,
}

/// Resolved playback info for downloading a track.
#[derive(Debug, Clone)]
pub(crate) enum PlaybackInfo {
    /// A single direct download URL.
    DirectUrl(String),
    /// Multiple segment URLs to concatenate (e.g. DASH).
    SegmentUrls(Vec<String>),
    /// A local file path that has already been downloaded.
    LocalFile(PathBuf),
}

/// Supplemental context for download sources that cannot resolve by track ID alone.
#[derive(Debug, Clone)]
pub(crate) struct DownloadTrackContext {
    pub artist_name: String,
    pub album_title: String,
    pub track_title: String,
    pub duration_secs: Option<u32>,
}

// ── Quality ─────────────────────────────────────────────────────────

/// Normalized quality level, provider-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Quality {
    Lossless,
    HiRes,
}

impl Quality {
    pub fn as_str(&self) -> &str {
        match self {
            Quality::Lossless => "LOSSLESS",
            Quality::HiRes => "HI_RES_LOSSLESS",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "HI_RES_LOSSLESS" | "HI_RES" | "HIRES" => Quality::HiRes,
            _ => Quality::Lossless,
        }
    }
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Traits ──────────────────────────────────────────────────────────

/// Provides metadata: artist search, album listing, track listing, image URLs.
#[async_trait]
pub(crate) trait MetadataProvider: Send + Sync {
    /// Unique provider identifier (e.g. "tidal", "musicbrainz", "deezer").
    fn id(&self) -> &str;

    /// Human-readable display name.
    #[allow(dead_code)]
    fn display_name(&self) -> &str;

    /// Search for artists by name.
    async fn search_artists(&self, query: &str) -> Result<Vec<ProviderArtist>, ProviderError>;

    /// Fetch all albums for an artist.
    async fn fetch_albums(
        &self,
        external_artist_id: &str,
    ) -> Result<Vec<ProviderAlbum>, ProviderError>;

    /// Fetch tracks for an album (with extra metadata for tagging).
    async fn fetch_tracks(
        &self,
        external_album_id: &str,
    ) -> Result<(Vec<ProviderTrack>, HashMap<String, Value>), ProviderError>;

    /// Fetch extra metadata for a single track (ISRC, BPM, key, etc.).
    async fn fetch_track_info_extra(
        &self,
        external_track_id: &str,
    ) -> Option<HashMap<String, Value>>;

    /// Validate an image ID before proxying. Returns `true` if safe.
    /// Override in provider implementations for provider-specific validation.
    fn validate_image_id(&self, image_id: &str) -> bool {
        let _ = image_id;
        true
    }

    /// Build the upstream image URL for a given image ref and size.
    fn image_url(&self, image_ref: &str, size: u16) -> String;

    /// Fetch cover art bytes for an image ref (full resolution).
    async fn fetch_cover_art_bytes(&self, image_ref: &str) -> Option<Vec<u8>>;

    /// Fetch a biographical summary for an artist (plain text).
    /// Default returns `None`; providers can override to source from Wikipedia etc.
    async fn fetch_artist_bio(&self, _external_artist_id: &str) -> Option<String> {
        None
    }
}

/// Provides track download (playback resolution).
#[async_trait]
pub(crate) trait DownloadSource: Send + Sync {
    /// Unique source identifier (e.g. "tidal").
    fn id(&self) -> &str;

    /// Whether this source requires provider-linked external IDs.
    fn requires_linked_provider(&self) -> bool {
        true
    }

    /// Resolve playback info (download URL / segments) for a track.
    async fn resolve_playback(
        &self,
        external_track_id: &str,
        quality: &Quality,
        context: Option<&DownloadTrackContext>,
    ) -> Result<PlaybackInfo, ProviderError>;
}
