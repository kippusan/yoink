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
use tracing::debug;

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
                        }
                    })
                    .collect();
                return (StatusCode::OK, Json(tracks)).into_response();
            }
            Err(err) => {
                debug!(
                    album_id = %album_id,
                    provider = %link.provider,
                    error = %err.0,
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
        return (StatusCode::BAD_REQUEST, "invalid size").into_response();
    }

    // Resolve upstream URL via the provider
    let Some(metadata_provider) = state.registry.metadata_provider(provider) else {
        return (StatusCode::BAD_REQUEST, "unknown provider").into_response();
    };

    // Provider-specific image ID validation
    if !metadata_provider.validate_image_id(image_id) {
        return (StatusCode::BAD_REQUEST, "invalid image id").into_response();
    }

    let upstream_url = metadata_provider.image_url(image_id, size);

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
                Ok(bytes) => (
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
                    .into_response(),
                Err(err) => {
                    debug!(error = %err, "Failed to read upstream image body");
                    (StatusCode::BAD_GATEWAY, "upstream read error").into_response()
                }
            }
        }
        Ok(upstream) => {
            debug!(status = %upstream.status(), url = %upstream_url, "Upstream image not found");
            (StatusCode::NOT_FOUND, "image not found").into_response()
        }
        Err(err) => {
            debug!(error = %err, "Failed to fetch upstream image");
            (StatusCode::BAD_GATEWAY, "upstream unreachable").into_response()
        }
    }
}
