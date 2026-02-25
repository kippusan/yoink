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

use crate::{
    models::*,
    services::{list_instances_payload, search_hifi_artists},
    state::AppState,
};

pub(crate) fn build_router(state: AppState) -> Router {
    Router::new()
        // ── API endpoints ───────────────────────────────────────
        .route("/api/library/artists", get(list_monitored_artists))
        .route("/api/library/albums", get(list_monitored_albums))
        .route("/api/downloads", get(list_download_jobs))
        .route("/api/instances", get(list_instances))
        .route("/api/albums/{album_id}/tracks", get(album_tracks))
        .route("/api/search", get(api_search))
        .route("/api/events", get(sse_events))
        .route("/api/image/{image_id}/{size}", get(proxy_tidal_image))
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

async fn list_instances(State(state): State<AppState>) -> impl IntoResponse {
    let payload = list_instances_payload(&state).await;
    Json(payload)
}

async fn album_tracks(
    State(state): State<AppState>,
    Path(album_id): Path<i64>,
) -> impl IntoResponse {
    use crate::models::{HifiAlbumItem, HifiAlbumResponse, TrackInfo};
    use crate::services::hifi::hifi_get_json;

    let result = hifi_get_json::<HifiAlbumResponse>(
        &state,
        "/album/",
        vec![("id".to_string(), album_id.to_string())],
    )
    .await;

    match result {
        Ok(response) => {
            let tracks: Vec<TrackInfo> = response
                .data
                .items
                .into_iter()
                .enumerate()
                .map(|(idx, item)| {
                    let track = match item {
                        HifiAlbumItem::Item { item } => item,
                        HifiAlbumItem::Track(t) => t,
                    };
                    let secs = track.duration.unwrap_or(0);
                    let mins = secs / 60;
                    let rem = secs % 60;
                    TrackInfo {
                        id: track.id,
                        title: track.title,
                        version: track.version,
                        track_number: track.track_number.unwrap_or((idx + 1) as u32),
                        duration_secs: secs,
                        duration_display: format!("{}:{:02}", mins, rem),
                    }
                })
                .collect();
            (StatusCode::OK, Json(tracks)).into_response()
        }
        Err(err) => {
            debug!(album_id, error = %err, "Failed to fetch album tracks");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": err})),
            )
                .into_response()
        }
    }
}

async fn api_search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    use crate::models::SearchResultArtist;
    use crate::ui::{artist_image_url, artist_profile_url};

    let q = match query.q.filter(|v| !v.trim().is_empty()) {
        Some(q) => q,
        None => return (StatusCode::OK, Json(Vec::<SearchResultArtist>::new())).into_response(),
    };

    let monitored = state.monitored_artists.read().await;
    let monitored_ids: std::collections::HashSet<i64> = monitored.iter().map(|a| a.id).collect();
    drop(monitored);

    match search_hifi_artists(&state, &q).await {
        Ok(artists) => {
            let results: Vec<SearchResultArtist> = artists
                .iter()
                .map(|a| SearchResultArtist {
                    id: a.id,
                    name: a.name.clone(),
                    picture_url: artist_image_url(a, 160),
                    tidal_url: artist_profile_url(a),
                    already_monitored: monitored_ids.contains(&a.id),
                })
                .collect();
            (StatusCode::OK, Json(results)).into_response()
        }
        Err(err) => {
            debug!(error = %err, "API search failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": err})),
            )
                .into_response()
        }
    }
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

async fn proxy_tidal_image(
    State(state): State<AppState>,
    Path((image_id, size)): Path<(String, u16)>,
) -> impl IntoResponse {
    // Validate inputs to prevent path traversal / abuse
    if !image_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') || image_id.len() > 60 {
        return (StatusCode::BAD_REQUEST, "invalid image id").into_response();
    }
    if ![160, 320, 640, 750, 1080].contains(&size) {
        return (StatusCode::BAD_REQUEST, "invalid size").into_response();
    }

    let upstream_url = format!(
        "https://resources.tidal.com/images/{}/{size}x{size}.jpg",
        image_id.replace('-', "/")
    );

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
