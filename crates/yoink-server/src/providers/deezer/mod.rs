use std::{collections::HashMap, num::NonZeroU32, time::Duration};

use async_trait::async_trait;
use governor::{Quota, RateLimiter, clock::DefaultClock, state::InMemoryState, state::NotKeyed};
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use super::{MetadataProvider, ProviderAlbum, ProviderArtist, ProviderError, ProviderTrack};

// ── Deezer API base ─────────────────────────────────────────────────

const DEEZER_API_BASE: &str = "https://api.deezer.com";

// ── API response models ─────────────────────────────────────────────

/// Wrapper for paginated list responses.
#[derive(Debug, Deserialize)]
struct DeezerList<T> {
    data: Vec<T>,
    #[allow(dead_code)]
    total: Option<u64>,
    next: Option<String>,
}

/// Top-level error object returned inside an otherwise-200 response.
#[derive(Debug, Deserialize)]
struct DeezerErrorBody {
    error: DeezerApiError,
}

#[derive(Debug, Deserialize)]
struct DeezerApiError {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
    code: i32,
}

#[derive(Debug, Deserialize)]
struct DeezerArtist {
    id: u64,
    name: String,
    /// MD5 hash for artist picture (may be absent).
    #[serde(default)]
    picture_medium: Option<String>,
    #[serde(default)]
    picture_big: Option<String>,
    #[serde(default)]
    nb_album: Option<u32>,
    #[serde(default)]
    nb_fan: Option<u64>,
    #[serde(default)]
    link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeezerAlbum {
    id: u64,
    title: String,
    #[serde(default)]
    record_type: Option<String>,
    /// "YYYY-MM-DD" or sometimes just "YYYY".
    #[serde(default)]
    release_date: Option<String>,
    /// MD5 hash for constructing CDN cover URLs.
    #[serde(default)]
    md5_image: Option<String>,
    #[serde(default)]
    explicit_lyrics: bool,
}

#[derive(Debug, Deserialize)]
struct DeezerTrack {
    id: u64,
    title: String,
    #[serde(default)]
    title_version: Option<String>,
    #[serde(default)]
    isrc: Option<String>,
    /// Duration in seconds.
    duration: u32,
    #[serde(default)]
    track_position: Option<u32>,
    #[serde(default)]
    disk_number: Option<u32>,
    #[serde(default)]
    explicit_lyrics: bool,
}

// ── DeezerProvider ──────────────────────────────────────────────────

pub(crate) struct DeezerProvider {
    http: reqwest::Client,
    rate_limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
}

impl DeezerProvider {
    pub fn new() -> Self {
        let user_agent = format!(
            "Yoink/{} (flyinpancake@pm.me)",
            env!("CARGO_PKG_VERSION")
        );

        let http = reqwest::Client::builder()
            .user_agent(&user_agent)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client for Deezer");

        // Deezer allows 50 requests per 5 seconds.
        let quota = Quota::with_period(Duration::from_millis(100))
            .expect("non-zero period")
            .allow_burst(NonZeroU32::new(50).unwrap());

        let rate_limiter = RateLimiter::direct(quota);

        Self { http, rate_limiter }
    }

    /// Send a rate-limited GET request and deserialise the JSON body.
    ///
    /// Checks for Deezer's "error inside 200" pattern before attempting
    /// to deserialise into `T`.
    async fn deezer_get<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, ProviderError> {
        self.rate_limiter.until_ready().await;

        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| ProviderError(format!("Deezer HTTP error: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ProviderError(format!("Deezer body read error: {e}")))?;

        // Deezer returns HTTP 200 even for errors — check the JSON body.
        if let Ok(err) = serde_json::from_str::<DeezerErrorBody>(&body) {
            return Err(ProviderError(format!(
                "Deezer API error ({}): {} [code {}]",
                err.error.error_type, err.error.message, err.error.code
            )));
        }

        if !status.is_success() {
            return Err(ProviderError(format!(
                "Deezer HTTP {status}: {body}"
            )));
        }

        serde_json::from_str::<T>(&body).map_err(|e| {
            ProviderError(format!("Deezer JSON parse error: {e}"))
        })
    }

    /// Build a Deezer API URL with query parameters (handles encoding).
    fn api_url(path: &str, params: &[(&str, &str)]) -> String {
        let mut url = format!("{DEEZER_API_BASE}{path}");
        if !params.is_empty() {
            url.push('?');
            for (i, (key, value)) in params.iter().enumerate() {
                if i > 0 {
                    url.push('&');
                }
                url.push_str(key);
                url.push('=');
                // Percent-encode the value for URL safety.
                for byte in value.bytes() {
                    match byte {
                        b'A'..=b'Z'
                        | b'a'..=b'z'
                        | b'0'..=b'9'
                        | b'-'
                        | b'_'
                        | b'.'
                        | b'~' => url.push(byte as char),
                        _ => {
                            url.push('%');
                            url.push_str(&format!("{byte:02X}"));
                        }
                    }
                }
            }
        }
        url
    }

    /// Fetch a paginated list, following `next` links until exhausted.
    async fn deezer_get_all<T: serde::de::DeserializeOwned>(
        &self,
        initial_url: &str,
    ) -> Result<Vec<T>, ProviderError> {
        let mut all = Vec::new();
        let mut url = initial_url.to_string();

        loop {
            let page: DeezerList<T> = self.deezer_get(&url).await?;
            all.extend(page.data);

            match page.next {
                Some(next_url) if !next_url.is_empty() => url = next_url,
                _ => break,
            }
        }

        Ok(all)
    }
}

/// Map Deezer `nb_fan` (fan count) to a 0–100 popularity value.
///
/// Uses a logarithmic scale so the huge range of fan counts compresses
/// into something useful:
///   0        → 0
///   ~100     → ~20
///   ~10 000  → ~50
///   ~1M      → ~75
///   ~100M    → ~100
fn fan_count_to_popularity(nb_fan: u64) -> u8 {
    if nb_fan == 0 {
        return 0;
    }
    let log_val = (nb_fan as f64).log10(); // log10(100M) ≈ 8
    let scaled = (log_val / 8.0 * 100.0).round() as u8;
    scaled.min(100)
}

/// Map Deezer `record_type` to the uppercase album type used internally.
fn map_record_type(record_type: &str) -> &str {
    match record_type {
        "album" => "ALBUM",
        "single" => "SINGLE",
        "ep" => "EP",
        "compile" => "COMPILATION",
        _ => "OTHER",
    }
}

// ── MetadataProvider impl ───────────────────────────────────────────

#[async_trait]
impl MetadataProvider for DeezerProvider {
    fn id(&self) -> &str {
        "deezer"
    }

    fn display_name(&self) -> &str {
        "Deezer"
    }

    async fn search_artists(&self, query: &str) -> Result<Vec<ProviderArtist>, ProviderError> {
        let url = Self::api_url("/search/artist", &[("q", query), ("limit", "25")]);
        let artists: DeezerList<DeezerArtist> = self.deezer_get(&url).await?;

        Ok(artists
            .data
            .into_iter()
            .map(|a| {
                // Extract md5 hash from picture URL and prefix with "artist:" to
                // distinguish from album cover hashes in image_url().
                let image_ref = extract_md5_from_picture_url(
                    a.picture_big.as_deref().or(a.picture_medium.as_deref()),
                )
                .map(|md5| format!("artist:{md5}"));

                let popularity = a.nb_fan.map(fan_count_to_popularity);
                let url = a
                    .link
                    .or_else(|| Some(format!("https://www.deezer.com/artist/{}", a.id)));

                let mut disambiguation_parts = Vec::new();
                if let Some(n) = a.nb_album {
                    disambiguation_parts.push(format!("{n} albums"));
                }
                if let Some(fans) = a.nb_fan {
                    if fans > 0 {
                        disambiguation_parts.push(format!("{} fans", format_fan_count(fans)));
                    }
                }
                let disambiguation = if disambiguation_parts.is_empty() {
                    None
                } else {
                    Some(disambiguation_parts.join(", "))
                };

                ProviderArtist {
                    external_id: a.id.to_string(),
                    name: a.name,
                    image_ref,
                    url,
                    disambiguation,
                    artist_type: None, // Deezer doesn't provide artist type
                    country: None,     // Deezer doesn't provide country
                    tags: Vec::new(),  // Deezer doesn't provide genre tags in search
                    popularity,
                }
            })
            .collect())
    }

    async fn fetch_albums(
        &self,
        external_artist_id: &str,
    ) -> Result<Vec<ProviderAlbum>, ProviderError> {
        let url = format!(
            "{DEEZER_API_BASE}/artist/{external_artist_id}/albums?limit=50"
        );
        let albums = self.deezer_get_all::<DeezerAlbum>(&url).await?;

        Ok(albums
            .into_iter()
            .map(|a| {
                let album_type = a
                    .record_type
                    .as_deref()
                    .map(|rt| map_record_type(rt).to_string());

                let url = Some(format!("https://www.deezer.com/album/{}", a.id));

                ProviderAlbum {
                    external_id: a.id.to_string(),
                    title: a.title,
                    album_type,
                    release_date: a.release_date,
                    cover_ref: a.md5_image,
                    url,
                    explicit: a.explicit_lyrics,
                }
            })
            .collect())
    }

    async fn fetch_tracks(
        &self,
        external_album_id: &str,
    ) -> Result<(Vec<ProviderTrack>, HashMap<String, Value>), ProviderError> {
        let url = format!(
            "{DEEZER_API_BASE}/album/{external_album_id}/tracks?limit=200"
        );
        let tracks = self.deezer_get_all::<DeezerTrack>(&url).await?;

        let album_extra = HashMap::new();

        let provider_tracks = tracks
            .into_iter()
            .map(|t| {
                let mut extra = HashMap::new();
                if t.explicit_lyrics {
                    extra.insert("explicit".to_string(), Value::Bool(true));
                }

                ProviderTrack {
                    external_id: t.id.to_string(),
                    title: t.title,
                    version: t.title_version.filter(|v| !v.is_empty()),
                    track_number: t.track_position.unwrap_or(1),
                    disc_number: t.disk_number,
                    duration_secs: t.duration,
                    isrc: t.isrc.filter(|s| !s.is_empty()),
                    extra,
                }
            })
            .collect();

        Ok((provider_tracks, album_extra))
    }

    async fn fetch_track_info_extra(
        &self,
        _external_track_id: &str,
    ) -> Option<HashMap<String, Value>> {
        // Deezer provides ISRC directly in track listings; no extra call needed.
        None
    }

    fn validate_image_id(&self, image_id: &str) -> bool {
        // Accept "artist:{md5}" or bare 32-char hex (album cover).
        let md5 = image_id.strip_prefix("artist:").unwrap_or(image_id);
        md5.len() == 32 && md5.chars().all(|c| c.is_ascii_hexdigit())
    }

    fn image_url(&self, image_ref: &str, size: u16) -> String {
        // Snap to reasonable Deezer CDN sizes.
        let sz = if size <= 56 {
            56
        } else if size <= 250 {
            250
        } else if size <= 500 {
            500
        } else {
            1000
        };

        // Distinguish artist pictures from album covers via "artist:" prefix.
        let (img_type, md5) = if let Some(stripped) = image_ref.strip_prefix("artist:") {
            ("artist", stripped)
        } else {
            ("cover", image_ref)
        };

        format!(
            "https://cdn-images.dzcdn.net/images/{img_type}/{md5}/{sz}x{sz}-000000-80-0-0.jpg"
        )
    }

    async fn fetch_cover_art_bytes(&self, image_ref: &str) -> Option<Vec<u8>> {
        // Determine image type from ref prefix.
        let (img_type, md5) = if let Some(stripped) = image_ref.strip_prefix("artist:") {
            ("artist", stripped)
        } else {
            ("cover", image_ref)
        };
        let url = format!(
            "https://cdn-images.dzcdn.net/images/{img_type}/{md5}/1000x1000-000000-80-0-0.jpg"
        );

        self.rate_limiter.until_ready().await;

        let resp = self.http.get(&url).send().await.ok()?;

        if !resp.status().is_success() {
            warn!(
                status = %resp.status(),
                image_ref,
                "Deezer CDN returned non-success for cover art"
            );
            return None;
        }

        resp.bytes().await.ok().map(|b| b.to_vec())
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Extract the MD5 hash from a Deezer CDN picture URL.
///
/// URLs look like: `https://cdn-images.dzcdn.net/images/artist/{md5}/{size}x{size}-...`
/// We extract the `{md5}` segment (32 hex chars).
fn extract_md5_from_picture_url(url: Option<&str>) -> Option<String> {
    let url = url?;
    // Split on '/' and find a 32-char hex segment.
    for segment in url.split('/') {
        if segment.len() == 32 && segment.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(segment.to_string());
        }
    }
    None
}

/// Format a fan count for display: "1.2M", "45K", "123", etc.
fn format_fan_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.0}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}
