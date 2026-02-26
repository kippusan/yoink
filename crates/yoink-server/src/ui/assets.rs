use crate::providers::ProviderArtist;

/// Image URL for a provider artist search result (proxied through our image endpoint).
pub(crate) fn artist_image_url(
    provider_id: &str,
    artist: &ProviderArtist,
    size: u16,
) -> Option<String> {
    artist
        .image_ref
        .as_deref()
        .map(|r| yoink_shared::provider_image_url(provider_id, r, size))
}

/// Profile URL for a provider artist search result.
pub(crate) fn artist_profile_url(artist: &ProviderArtist) -> Option<String> {
    artist.url.clone()
}
