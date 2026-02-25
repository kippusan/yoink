use crate::models::*;
use yoink::shared::tidal_image_url;

/// Image URL for a HifiArtist search result (server-only; HifiArtist is not shared with WASM).
pub(crate) fn artist_image_url(artist: &HifiArtist, size: u16) -> Option<String> {
    artist
        .picture
        .as_deref()
        .or(artist.selected_album_cover_fallback.as_deref())
        .map(|id| tidal_image_url(id, size))
}

/// Tidal profile URL for a HifiArtist search result.
pub(crate) fn artist_profile_url(artist: &HifiArtist) -> String {
    artist
        .url
        .clone()
        .unwrap_or_else(|| format!("https://tidal.com/artist/{}", artist.id))
}
