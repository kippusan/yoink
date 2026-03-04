use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use tokio_stream::{StreamExt as _, wrappers::BroadcastStream};
use tracing::{debug, warn};

use uuid::Uuid;

use crate::{db, models::*, state::AppState};

pub(crate) fn build_router(state: AppState) -> Router {
    Router::new()
        // ── API endpoints ───────────────────────────────────────
        .route("/api/library/artists", get(list_monitored_artists))
        .route("/api/library/albums", get(list_monitored_albums))
        .route("/api/downloads", get(list_download_jobs))
        .route("/api/tidal/instances", get(list_tidal_instances))
        .route("/api/albums/{album_id}/tracks", get(album_tracks))
        .route("/api/search", get(api_search))
        .route("/api/search/albums", get(api_search_albums))
        .route("/api/search/tracks", get(api_search_tracks))
        .route("/api/events", get(sse_events))
        .route("/api/image/{image_id}/{size}", get(proxy_tidal_image))
        .route(
            "/api/image/{provider}/{image_id}/{size}",
            get(proxy_provider_image),
        )
        .with_state(state)
}

// ── API handlers ────────────────────────────────────────────────────

async fn list_monitored_artists(State(state): State<AppState>) -> impl IntoResponse {
    let artists = state.monitored_artists.read().await.clone();
    Json(artists)
}

async fn list_monitored_albums(State(state): State<AppState>) -> impl IntoResponse {
    let albums = state.monitored_albums.read().await.clone();
    Json(albums)
}

async fn list_download_jobs(State(state): State<AppState>) -> impl IntoResponse {
    let jobs = state.download_jobs.read().await.clone();
    Json(jobs)
}

async fn list_tidal_instances(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(tidal) = state.registry.tidal_provider() {
        let payload = tidal.list_instances_payload().await;
        return Json(serde_json::to_value(payload).unwrap_or_default()).into_response();
    }
    Json(serde_json::json!({"error": "Tidal provider not available"})).into_response()
}

async fn album_tracks(
    State(state): State<AppState>,
    Path(album_id): Path<Uuid>,
) -> impl IntoResponse {
    // First try loading from local DB
    match db::load_tracks_for_album(&state.db, album_id).await {
        Ok(tracks) if !tracks.is_empty() => {
            return (StatusCode::OK, Json(tracks)).into_response();
        }
        _ => {}
    }

    // Fallback: fetch from any available metadata provider via provider link
    let links = match db::load_album_provider_links(&state.db, album_id).await {
        Ok(links) => links,
        Err(err) => {
            debug!(album_id = %album_id, error = %err, "Failed to load album provider links");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": err.to_string()})),
            )
                .into_response();
        }
    };

    // Try each provider link until one succeeds
    for link in &links {
        let Some(provider) = state.registry.metadata_provider(&link.provider) else {
            continue;
        };

        match provider.fetch_tracks(&link.external_id).await {
            Ok((provider_tracks, _album_extra)) => {
                let tracks: Vec<TrackInfo> = provider_tracks
                    .into_iter()
                    .map(|t| {
                        let secs = t.duration_secs;
                        let mins = secs / 60;
                        let rem = secs % 60;
                        TrackInfo {
                            id: Uuid::now_v7(),
                            title: t.title,
                            version: t.version,
                            disc_number: t.disc_number.unwrap_or(1),
                            track_number: t.track_number,
                            duration_secs: secs,
                            duration_display: format!("{}:{:02}", mins, rem),
                            isrc: t.isrc,
                            explicit: t.explicit,
                            track_artist: t.artists,
                            file_path: None,
                            monitored: false,
                            acquired: false,
                        }
                    })
                    .collect();
                return (StatusCode::OK, Json(tracks)).into_response();
            }
            Err(err) => {
                debug!(
                    album_id = %album_id,
                    provider = %link.provider,
                    error = %err,
                    "Failed to fetch tracks from provider"
                );
            }
        }
    }

    // No provider could serve the tracks
    (StatusCode::OK, Json(Vec::<TrackInfo>::new())).into_response()
}

async fn api_search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    use crate::ui::{artist_image_url, artist_profile_url};

    let q = match query.q.filter(|v| !v.trim().is_empty()) {
        Some(q) => q,
        None => return (StatusCode::OK, Json(Vec::<SearchResultArtist>::new())).into_response(),
    };

    // Check which names are already monitored
    let monitored = state.monitored_artists.read().await;
    let monitored_names: std::collections::HashSet<String> = monitored
        .iter()
        .map(|a| a.name.to_ascii_lowercase())
        .collect();
    drop(monitored);

    // Fan-out search to all providers
    let all_results = state.registry.search_artists_all(&q).await;
    let mut results = Vec::new();

    for (provider_id, artists) in all_results {
        for a in &artists {
            results.push(SearchResultArtist {
                provider: provider_id.clone(),
                external_id: a.external_id.clone(),
                name: a.name.clone(),
                image_url: artist_image_url(&provider_id, a, 160),
                url: artist_profile_url(a),
                already_monitored: monitored_names.contains(&a.name.to_ascii_lowercase()),
                disambiguation: a.disambiguation.clone(),
                artist_type: a.artist_type.clone(),
                country: a.country.clone(),
                tags: a.tags.clone(),
                popularity: a.popularity,
            });
        }
    }

    (StatusCode::OK, Json(results)).into_response()
}

async fn api_search_albums(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let q = match query.q.filter(|v| !v.trim().is_empty()) {
        Some(q) => q,
        None => return (StatusCode::OK, Json(Vec::<SearchResultAlbum>::new())).into_response(),
    };

    let all_results = state.registry.search_albums_all(&q).await;
    let mut results = Vec::new();

    for (provider_id, albums) in all_results {
        for a in albums {
            let cover_url = a
                .cover_ref
                .as_deref()
                .map(|c| yoink_shared::provider_image_url(&provider_id, c, 320));

            results.push(SearchResultAlbum {
                provider: provider_id.clone(),
                external_id: a.external_id,
                title: a.title,
                album_type: a.album_type,
                release_date: a.release_date,
                cover_url,
                url: a.url,
                explicit: a.explicit,
                artist_name: a.artist_name,
                artist_external_id: a.artist_external_id,
            });
        }
    }

    (StatusCode::OK, Json(results)).into_response()
}

async fn api_search_tracks(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let q = match query.q.filter(|v| !v.trim().is_empty()) {
        Some(q) => q,
        None => return (StatusCode::OK, Json(Vec::<SearchResultTrack>::new())).into_response(),
    };

    let all_results = state.registry.search_tracks_all(&q).await;
    let mut results = Vec::new();

    for (provider_id, tracks) in all_results {
        for t in tracks {
            let secs = t.duration_secs;
            let mins = secs / 60;
            let rem = secs % 60;

            let album_cover_url = t
                .album_cover_ref
                .as_deref()
                .map(|c| yoink_shared::provider_image_url(&provider_id, c, 160));

            results.push(SearchResultTrack {
                provider: provider_id.clone(),
                external_id: t.external_id,
                title: t.title,
                version: t.version,
                duration_secs: t.duration_secs,
                duration_display: format!("{mins}:{rem:02}"),
                isrc: t.isrc,
                explicit: t.explicit,
                artist_name: t.artist_name,
                artist_external_id: t.artist_external_id,
                album_title: t.album_title,
                album_external_id: t.album_external_id,
                album_cover_url,
            });
        }
    }

    (StatusCode::OK, Json(results)).into_response()
}

async fn sse_events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(()) => Some(Ok(Event::default().event("update").data("refresh"))),
        Err(_) => None, // lagged — skip
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ── Image proxy ─────────────────────────────────────────────────────

/// Legacy image proxy route: /api/image/{image_id}/{size}
/// Assumes Tidal image format for backwards compatibility.
async fn proxy_tidal_image(
    State(state): State<AppState>,
    Path((image_id, size)): Path<(String, u16)>,
) -> impl IntoResponse {
    proxy_image_impl(&state, "tidal", &image_id, size).await
}

/// Provider-aware image proxy: /api/image/{provider}/{image_id}/{size}
async fn proxy_provider_image(
    State(state): State<AppState>,
    Path((provider, image_id, size)): Path<(String, String, u16)>,
) -> impl IntoResponse {
    proxy_image_impl(&state, &provider, &image_id, size).await
}

async fn proxy_image_impl(
    state: &AppState,
    provider: &str,
    image_id: &str,
    size: u16,
) -> axum::response::Response {
    // Validate size
    if ![160, 320, 640, 750, 1080].contains(&size) {
        debug!(
            provider,
            image_id, size, "Image proxy rejected: invalid size"
        );
        return (StatusCode::BAD_REQUEST, "invalid size").into_response();
    }

    // Resolve upstream URL via the provider
    let Some(metadata_provider) = state.registry.metadata_provider(provider) else {
        debug!(provider, image_id, "Image proxy rejected: unknown provider");
        return (StatusCode::BAD_REQUEST, "unknown provider").into_response();
    };

    // Provider-specific image ID validation
    if !metadata_provider.validate_image_id(image_id) {
        debug!(provider, image_id, "Image proxy rejected: invalid image id");
        return (StatusCode::BAD_REQUEST, "invalid image id").into_response();
    }

    let upstream_url = metadata_provider.image_url(image_id, size);
    debug!(provider, image_id, size, %upstream_url, "Image proxy fetching upstream");

    let resp = state
        .http
        .get(&upstream_url)
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    match resp {
        Ok(upstream) if upstream.status().is_success() => {
            let content_type = upstream
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("image/jpeg")
                .to_string();
            match upstream.bytes().await {
                Ok(bytes) => {
                    debug!(
                        provider,
                        image_id,
                        size,
                        bytes = bytes.len(),
                        "Image proxy success"
                    );
                    (
                        StatusCode::OK,
                        [
                            (header::CONTENT_TYPE, content_type),
                            (
                                header::CACHE_CONTROL,
                                "public, max-age=86400, immutable".to_string(),
                            ),
                        ],
                        bytes,
                    )
                        .into_response()
                }
                Err(err) => {
                    warn!(provider, image_id, %upstream_url, error = %err, "Image proxy: failed to read upstream body");
                    (StatusCode::BAD_GATEWAY, "upstream read error").into_response()
                }
            }
        }
        Ok(upstream) => {
            let status = upstream.status();
            warn!(provider, image_id, size, %upstream_url, %status, "Image proxy: upstream returned non-success");
            (StatusCode::NOT_FOUND, "image not found").into_response()
        }
        Err(err) => {
            warn!(provider, image_id, %upstream_url, error = %err, "Image proxy: upstream unreachable");
            (StatusCode::BAD_GATEWAY, "upstream unreachable").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::models::DownloadStatus;
    use crate::providers::registry::ProviderRegistry;
    use crate::providers::ProviderArtist;
    use crate::test_helpers::*;

    use super::build_router;

    /// Helper: send a GET request to a path and return the status + body bytes.
    async fn get(
        state: crate::state::AppState,
        path: &str,
    ) -> (StatusCode, Vec<u8>) {
        let app = build_router(state);
        let req = Request::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec();
        (status, body)
    }

    // ── GET /api/library/artists ────────────────────────────────

    #[tokio::test]
    async fn list_artists_empty() {
        let (state, _tmp) = test_app_state().await;
        let (status, body) = get(state, "/api/library/artists").await;
        assert_eq!(status, StatusCode::OK);
        let artists: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(artists.is_empty());
    }

    #[tokio::test]
    async fn list_artists_with_data() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Test Artist").await;
        state.monitored_artists.write().await.push(artist.clone());

        let (status, body) = get(state, "/api/library/artists").await;
        assert_eq!(status, StatusCode::OK);
        let artists: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0]["name"], "Test Artist");
    }

    // ── GET /api/library/albums ─────────────────────────────────

    #[tokio::test]
    async fn list_albums_returns_correct_json() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Artist").await;
        let album = seed_album(&state.db, artist.id, "My Album").await;
        state.monitored_albums.write().await.push(album.clone());

        let (status, body) = get(state, "/api/library/albums").await;
        assert_eq!(status, StatusCode::OK);
        let albums: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0]["title"], "My Album");
        assert_eq!(albums[0]["monitored"], true);
    }

    // ── GET /api/downloads ──────────────────────────────────────

    #[tokio::test]
    async fn list_downloads_empty() {
        let (state, _tmp) = test_app_state().await;
        let (status, body) = get(state, "/api/downloads").await;
        assert_eq!(status, StatusCode::OK);
        let jobs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(jobs.is_empty());
    }

    #[tokio::test]
    async fn list_downloads_with_jobs() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Artist").await;
        let album = seed_album(&state.db, artist.id, "Album").await;
        let job = seed_job(&state.db, album.id, DownloadStatus::Queued).await;
        state.download_jobs.write().await.push(job);

        let (status, body) = get(state, "/api/downloads").await;
        assert_eq!(status, StatusCode::OK);
        let jobs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["status"], "queued");
    }

    // ── GET /api/albums/{id}/tracks ─────────────────────────────

    #[tokio::test]
    async fn album_tracks_from_db() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Artist").await;
        let album = seed_album(&state.db, artist.id, "Album").await;
        seed_tracks(&state.db, album.id, 3).await;

        let (status, body) = get(state, &format!("/api/albums/{}/tracks", album.id)).await;
        assert_eq!(status, StatusCode::OK);
        let tracks: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0]["title"], "Track 1");
    }

    #[tokio::test]
    async fn album_tracks_empty_when_no_tracks() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Artist").await;
        let album = seed_album(&state.db, artist.id, "Album").await;

        let (status, body) = get(state, &format!("/api/albums/{}/tracks", album.id)).await;
        assert_eq!(status, StatusCode::OK);
        let tracks: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(tracks.is_empty());
    }

    // ── GET /api/search?q= ──────────────────────────────────────

    #[tokio::test]
    async fn search_empty_query_returns_empty() {
        let (state, _tmp) = test_app_state().await;
        let (status, body) = get(state, "/api/search?q=").await;
        assert_eq!(status, StatusCode::OK);
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_with_mock_provider_returns_results() {
        let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
        *mock.search_artists_result.lock().await = Ok(vec![ProviderArtist {
            external_id: "EXT1".to_string(),
            name: "Found Artist".to_string(),
            image_ref: None,
            url: Some("https://example.com/artist".to_string()),
            disambiguation: Some("Rock band".to_string()),
            artist_type: Some("Group".to_string()),
            country: Some("US".to_string()),
            tags: vec!["rock".to_string()],
            popularity: Some(80),
        }]);

        let mut registry = ProviderRegistry::new();
        registry.register_metadata(mock as Arc<dyn crate::providers::MetadataProvider>);

        let (state, _tmp) = test_app_state_with_registry(registry).await;

        let (status, body) = get(state, "/api/search?q=Found").await;
        assert_eq!(status, StatusCode::OK);
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], "Found Artist");
        assert_eq!(results[0]["provider"], "mock_prov");
        assert_eq!(results[0]["already_monitored"], false);
    }

    #[tokio::test]
    async fn search_flags_already_monitored() {
        let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
        *mock.search_artists_result.lock().await = Ok(vec![ProviderArtist {
            external_id: "E1".to_string(),
            name: "Monitored One".to_string(),
            image_ref: None,
            url: None,
            disambiguation: None,
            artist_type: None,
            country: None,
            tags: vec![],
            popularity: None,
        }]);

        let mut registry = ProviderRegistry::new();
        registry.register_metadata(mock as Arc<dyn crate::providers::MetadataProvider>);

        let (state, _tmp) = test_app_state_with_registry(registry).await;

        // Add "Monitored One" to the in-memory list
        let artist = seed_artist(&state.db, "Monitored One").await;
        state.monitored_artists.write().await.push(artist);

        let (status, body) = get(state, "/api/search?q=Monitored").await;
        assert_eq!(status, StatusCode::OK);
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["already_monitored"], true);
    }

    // ── GET /api/tidal/instances ─────────────────────────────────

    #[tokio::test]
    async fn tidal_instances_no_tidal() {
        let (state, _tmp) = test_app_state().await;
        let (status, body) = get(state, "/api/tidal/instances").await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].is_string()); // "Tidal provider not available"
    }

    // ── GET /api/image/{provider}/{id}/{size} ────────────────────

    #[tokio::test]
    async fn image_proxy_invalid_size() {
        let mock = Arc::new(MockMetadataProvider::new("mock_prov"));
        let mut registry = ProviderRegistry::new();
        registry.register_metadata(mock as Arc<dyn crate::providers::MetadataProvider>);

        let (state, _tmp) = test_app_state_with_registry(registry).await;

        let (status, _) = get(state, "/api/image/mock_prov/abc123/999").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn image_proxy_unknown_provider() {
        let (state, _tmp) = test_app_state().await;
        let (status, _) = get(state, "/api/image/nonexistent/abc123/320").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
