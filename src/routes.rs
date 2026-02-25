use std::{convert::Infallible, time::Duration};

use axum::{
    extract::{Form, Path, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Redirect,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use tokio_stream::{wrappers::BroadcastStream, StreamExt as _};
use tracing::{debug, info};

use crate::{
    db,
    models::*,
    services::{
        enqueue_album_download, list_instances_payload, retag_existing_files, scan_and_import_library,
        search_hifi_artists, sync_artist_albums_from_hifi, update_wanted,
    },
    state::AppState,
};

/// Sanitize return_to to only allow local paths (prevent open redirect)
fn safe_redirect(return_to: Option<String>, fallback: &str) -> String {
    match return_to {
        Some(path) if path.starts_with('/') && !path.starts_with("//") => path,
        _ => fallback.to_string(),
    }
}

pub(crate) fn build_router(state: AppState) -> Router {
    Router::new()
        // ── Pages ───────────────────────────────────────────────
        // /, /artists, /artists/:id, /wanted are now handled by the Leptos App
        // .route("/", get(dashboard_page))
        // .route("/artists", get(artists_page))
        // .route("/artists/{artist_id}", get(artist_detail_page))
        // /wanted is now handled by the Leptos App component
        // .route("/wanted", get(wanted_page))
        // ── Form actions ────────────────────────────────────────
        .route("/artists/add", post(add_artist))
        .route("/artists/remove", post(remove_artist))
        .route("/artists/sync", post(sync_artist_albums))
        .route("/albums/monitor", post(toggle_album_monitor))
        .route("/albums/bulk-monitor", post(bulk_monitor_albums))
        .route("/downloads/retry", post(retry_download))
        .route("/downloads/cancel", post(cancel_download))
        .route("/downloads/clear", post(clear_completed_downloads))
        .route("/library/retag", post(retag_library))
        .route("/library/scan-import", post(scan_import_library))
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

// ── Form action handlers ────────────────────────────────────────────

async fn add_artist(
    State(state): State<AppState>,
    Form(form): Form<AddArtistForm>,
) -> impl IntoResponse {
    {
        let mut artists = state.monitored_artists.write().await;
        if artists.iter().all(|artist| artist.id != form.id) {
            let artist = MonitoredArtist {
                id: form.id,
                name: form.name,
                picture: form.picture.filter(|s| !s.is_empty()),
                tidal_url: form.tidal_url.filter(|s| !s.is_empty()),
                quality_profile: state.default_quality.clone(),
                added_at: Utc::now(),
            };
            let _ = db::upsert_artist(&state.db, &artist).await;
            artists.push(artist);
        }
    }

    let _ = sync_artist_albums_from_hifi(&state, form.id).await;
    Redirect::to(&safe_redirect(form.return_to, "/artists"))
}

async fn sync_artist_albums(
    State(state): State<AppState>,
    Form(form): Form<SyncArtistAlbumsForm>,
) -> impl IntoResponse {
    let _ = sync_artist_albums_from_hifi(&state, form.artist_id).await;
    Redirect::to(&safe_redirect(form.return_to, "/artists"))
}

async fn toggle_album_monitor(
    State(state): State<AppState>,
    Form(form): Form<ToggleAlbumMonitorForm>,
) -> impl IntoResponse {
    let mut album_to_queue = None;
    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(album) = albums.iter_mut().find(|album| album.id == form.album_id) {
            album.monitored = form.monitored;
            update_wanted(album);
            let _ = db::update_album_flags(&state.db, album.id, album.monitored, album.acquired, album.wanted).await;
            if album.monitored && !album.acquired {
                album_to_queue = Some(album.clone());
            }
        }
    }

    if let Some(album) = album_to_queue {
        enqueue_album_download(&state, &album).await;
    }
    Redirect::to(&safe_redirect(form.return_to, "/artists"))
}

async fn remove_artist(
    State(state): State<AppState>,
    Form(form): Form<RemoveArtistForm>,
) -> impl IntoResponse {
    {
        let _ = db::delete_albums_by_artist(&state.db, form.artist_id).await;
        let _ = db::delete_artist(&state.db, form.artist_id).await;
    }
    {
        let mut albums = state.monitored_albums.write().await;
        albums.retain(|a| a.artist_id != form.artist_id);
    }
    {
        let mut artists = state.monitored_artists.write().await;
        artists.retain(|a| a.id != form.artist_id);
    }
    info!(artist_id = form.artist_id, "Removed artist and their albums");
    Redirect::to(&safe_redirect(form.return_to, "/artists"))
}

async fn bulk_monitor_albums(
    State(state): State<AppState>,
    Form(form): Form<BulkMonitorForm>,
) -> impl IntoResponse {
    let mut to_queue = Vec::new();
    {
        let mut albums = state.monitored_albums.write().await;
        for album in albums.iter_mut().filter(|a| a.artist_id == form.artist_id) {
            album.monitored = form.monitored;
            update_wanted(album);
            let _ = db::update_album_flags(&state.db, album.id, album.monitored, album.acquired, album.wanted).await;
            if album.monitored && !album.acquired {
                to_queue.push(album.clone());
            }
        }
    }
    for album in to_queue {
        enqueue_album_download(&state, &album).await;
    }
    let fallback = format!("/artists/{}", form.artist_id);
    Redirect::to(&safe_redirect(form.return_to, &fallback))
}

async fn cancel_download(
    State(state): State<AppState>,
    Form(form): Form<CancelDownloadForm>,
) -> impl IntoResponse {
    let mut jobs = state.download_jobs.write().await;
    if let Some(job) = jobs.iter_mut().find(|j| j.id == form.job_id) {
        if matches!(job.status, DownloadStatus::Queued) {
            job.status = DownloadStatus::Failed;
            job.error = Some("Cancelled by user".to_string());
            job.updated_at = Utc::now();
            let _ = db::update_job(&state.db, job).await;
            info!(job_id = form.job_id, "Cancelled download job");
        }
    }
    Redirect::to(&safe_redirect(form.return_to, "/"))
}

async fn clear_completed_downloads(
    State(state): State<AppState>,
    Form(form): Form<ClearCompletedForm>,
) -> impl IntoResponse {
    {
        let _ = db::delete_completed_jobs(&state.db).await;
    }
    {
        let mut jobs = state.download_jobs.write().await;
        jobs.retain(|j| j.status != DownloadStatus::Completed);
    }
    info!("Cleared completed download jobs");
    Redirect::to(&safe_redirect(form.return_to, "/"))
}

async fn retag_library(
    State(state): State<AppState>,
    Form(form): Form<RetagLibraryForm>,
) -> impl IntoResponse {
    let redirect_to = safe_redirect(form.return_to, "/");
    let worker_state = state.clone();
    tokio::spawn(async move {
        match retag_existing_files(&worker_state).await {
            Ok((tagged, missing, albums)) => {
                info!(
                    tagged_files = tagged,
                    missing_files = missing,
                    scanned_albums = albums,
                    "Completed manual library retag"
                );
            }
            Err(err) => {
                info!(error = %err, "Library retag failed");
            }
        }
    });
    Redirect::to(&redirect_to)
}

async fn scan_import_library(
    State(state): State<AppState>,
    Form(form): Form<ScanImportLibraryForm>,
) -> impl IntoResponse {
    let redirect_to = safe_redirect(form.return_to, "/");
    let worker_state = state.clone();
    tokio::spawn(async move {
        match scan_and_import_library(&worker_state).await {
            Ok(summary) => {
                info!(
                    discovered = summary.discovered_albums,
                    imported = summary.imported_albums,
                    artists_added = summary.artists_added,
                    unmatched = summary.unmatched_albums,
                    "Completed scan/import pass"
                );
            }
            Err(err) => {
                info!(error = %err, "Scan/import failed");
            }
        }
    });
    Redirect::to(&redirect_to)
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

async fn retry_download(
    State(state): State<AppState>,
    Form(form): Form<RetryDownloadForm>,
) -> impl IntoResponse {
    let redirect_to = safe_redirect(form.return_to.clone(), "/wanted");
    {
        let mut jobs = state.download_jobs.write().await;
        if let Some(job) = jobs
            .iter_mut()
            .find(|job| job.album_id == form.album_id && job.status == DownloadStatus::Failed)
        {
            job.status = DownloadStatus::Queued;
            job.error = None;
            job.updated_at = Utc::now();
            let _ = db::update_job(&state.db, job).await;
            info!(album_id = form.album_id, job_id = job.id, "Retrying failed download job");
            state.download_notify.notify_one();
            return Redirect::to(&redirect_to);
        }
    }

    let album = {
        let albums = state.monitored_albums.read().await;
        albums.iter().find(|album| album.id == form.album_id).cloned()
    };

    if let Some(album) = album {
        info!(album_id = album.id, title = %album.title, "Creating retry download job");
        enqueue_album_download(&state, &album).await;
    }

    Redirect::to(&redirect_to)
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
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": err}))).into_response()
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
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": err}))).into_response()
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
    if !image_id
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '-')
        || image_id.len() > 60
    {
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
