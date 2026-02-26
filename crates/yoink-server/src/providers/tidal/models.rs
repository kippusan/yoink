use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Tidal API response types ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct HifiResponse {
    pub data: SearchData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiArtistAlbumsResponse {
    pub albums: HifiAlbumPage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiAlbumResponse {
    pub data: HifiAlbumData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiAlbumData {
    pub items: Vec<HifiAlbumItem>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum HifiAlbumItem {
    Item { item: HifiTrack },
    Track(HifiTrack),
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiAlbumPage {
    pub items: Vec<HifiAlbum>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HifiAlbum {
    pub id: i64,
    pub title: String,
    #[serde(rename = "type")]
    pub album_type: Option<String>,
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
    pub cover: Option<String>,
    pub url: Option<String>,
    pub explicit: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HifiTrack {
    pub id: i64,
    pub title: String,
    pub version: Option<String>,
    #[serde(rename = "trackNumber")]
    pub track_number: Option<u32>,
    pub duration: Option<u32>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HifiArtist {
    pub id: i64,
    pub name: String,
    pub picture: Option<String>,
    #[serde(rename = "selectedAlbumCoverFallback")]
    pub selected_album_cover_fallback: Option<String>,
    pub url: Option<String>,
    pub popularity: Option<u8>,
    #[serde(rename = "artistRoles", default)]
    pub artist_roles: Vec<HifiArtistRole>,
    #[serde(rename = "artistTypes", default)]
    pub artist_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HifiArtistRole {
    pub category: Option<String>,
}

// ── Search wrappers ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct SearchData {
    pub artists: Option<PagedArtists>,
    pub items: Option<Vec<HifiArtist>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PagedArtists {
    pub items: Vec<HifiArtist>,
}

// ── Playback / manifest ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct HifiPlaybackResponse {
    pub data: HifiPlaybackData,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HifiPlaybackData {
    #[serde(rename = "manifestMimeType")]
    pub manifest_mime_type: String,
    pub manifest: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BtsManifest {
    pub urls: Vec<String>,
}

// ── Instance discovery ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FeedInstance {
    pub url: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DownInstance {
    pub url: String,
    pub status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RankedInstance {
    pub url: String,
    pub version: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UptimeFeed {
    pub api: Vec<FeedInstance>,
    pub streaming: Vec<FeedInstance>,
    pub down: Vec<DownInstance>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InstancesResponse {
    pub manual_override: Option<String>,
    pub active_base_url: Option<String>,
    pub last_refresh: Option<DateTime<Utc>>,
    pub ranked: Vec<RankedInstance>,
    pub api: Vec<FeedInstance>,
    pub streaming: Vec<FeedInstance>,
    pub down: Vec<DownInstance>,
}
