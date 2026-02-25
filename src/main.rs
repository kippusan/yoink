mod app_config;
mod config;
mod db;
mod logging;
mod models;
mod routes;
mod services;
mod state;
mod ui;

use std::time::Duration;

use axum::routing::{get, get_service};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};

use crate::{
    app_config::AppConfig,
    config::QUALITY_WARNING,
    logging::init_logging,
    routes::build_router,
    services::{download_worker_loop, reconcile_library_files},
    state::AppState,
};

use yoink::app::{App, shell::shell};

#[tokio::main]
async fn main() {
    let app_config = AppConfig::from_env().unwrap_or_else(|err| {
        panic!("Failed to parse configuration from environment: {err}");
    });

    init_logging(&app_config.log_format);

    // Initialise the Leptos/Tokio executor so SSR rendering can spawn futures.
    // generate_route_list calls any_spawner::Executor::init_tokio() internally.
    let _leptos_routes = leptos_axum::generate_route_list(App);

    let manual_hifi_base_url = Some(app_config.hifi_api_base_url.clone());
    let music_root = app_config.music_root_path();
    let default_quality = app_config.default_quality.clone();
    let default_quality_for_log = default_quality.clone();
    let quality_warning_for_log = if default_quality_for_log == "LOSSLESS" {
        QUALITY_WARNING
    } else {
        ""
    };
    let music_root_for_log = music_root.display().to_string();

    let db_url = app_config.database_url.clone();
    let db_url_for_log = db_url.clone();

    let state = AppState::new(
        manual_hifi_base_url,
        music_root,
        default_quality,
        app_config.download_lyrics,
        &db_url,
    )
    .await;
    let download_lyrics_for_log = state.download_lyrics;
    if let Err(err) = reconcile_library_files(&state).await {
        warn!(error = %err, "Initial library reconciliation failed");
    }
    let site_root = app_config.leptos_site_root.clone();

    let worker_state = state.clone();
    tokio::spawn(async move {
        download_worker_loop(worker_state).await;
    });

    let reconcile_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(45));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(err) = reconcile_library_files(&reconcile_state).await {
                warn!(error = %err, "Periodic library reconciliation failed");
            }
        }
    });

    // Build the Leptos server context from AppState.
    // This lightweight clone shares the same Arc<RwLock<..>> data.
    let search_state = state.clone();
    let search_fn: yoink::shared::SearchArtistsFn = std::sync::Arc::new(move |query: String| {
        let s = search_state.clone();
        Box::pin(async move {
            match services::search_hifi_artists(&s, &query).await {
                Ok(artists) => Ok(artists
                    .into_iter()
                    .map(|a| yoink::shared::SearchArtistResult {
                        id: a.id,
                        name: a.name,
                        picture: a.picture.or(a.selected_album_cover_fallback),
                        url: a.url,
                    })
                    .collect()),
                Err(err) => Err(format!("Search failed: {err}")),
            }
        })
    });

    let tracks_state = state.clone();
    let fetch_tracks_fn: yoink::shared::FetchTracksFn =
        std::sync::Arc::new(move |album_id: i64| {
            let s = tracks_state.clone();
            Box::pin(async move {
                use models::{HifiAlbumItem, HifiAlbumResponse};
                use services::hifi::hifi_get_json;

                let response = hifi_get_json::<HifiAlbumResponse>(
                    &s,
                    "/album/",
                    vec![("id".to_string(), album_id.to_string())],
                )
                .await
                .map_err(|e| format!("Failed to fetch tracks: {e}"))?;

                Ok(response
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
                        yoink::shared::TrackInfo {
                            id: track.id,
                            title: track.title,
                            track_number: track.track_number.unwrap_or((idx + 1) as u32),
                            duration_secs: secs,
                            duration_display: format!("{mins}:{rem:02}"),
                        }
                    })
                    .collect())
            })
        });

    let action_state = state.clone();
    let dispatch_action_fn: yoink::shared::DispatchActionFn =
        std::sync::Arc::new(move |action: yoink::shared::ServerAction| {
            let s = action_state.clone();
            Box::pin(async move { dispatch_action_impl(s, action).await })
        });

    let server_ctx = yoink::shared::ServerContext {
        monitored_artists: state.monitored_artists.clone(),
        monitored_albums: state.monitored_albums.clone(),
        download_jobs: state.download_jobs.clone(),
        search_artists: search_fn,
        fetch_tracks: fetch_tracks_fn,
        dispatch_action: dispatch_action_fn,
    };

    // Closure that injects ServerContext — used for both SSR rendering
    // and server function handlers so they see the same context.
    let provide_server_ctx = {
        let ctx = server_ctx.clone();
        move || {
            leptos::context::provide_context(ctx.clone());
        }
    };

    // Server function handler — handles POST (and GET) calls from the WASM client.
    let server_fn_ctx = provide_server_ctx.clone();
    let server_fn_handler = move |req: axum::http::Request<axum::body::Body>| async move {
        leptos_axum::handle_server_fns_with_context(server_fn_ctx, req).await
    };

    // Old Axum routes + new Leptos-rendered pages.
    // render_app_to_stream_with_context injects ServerContext into Leptos context
    // so #[server] functions can access data via use_context::<ServerContext>().
    let leptos_handler = || {
        let ctx = provide_server_ctx.clone();
        get(leptos_axum::render_app_to_stream_with_context(ctx, shell))
    };

    let app = build_router(state)
        .route("/leptos/{*fn_name}", get(server_fn_handler.clone()).post(server_fn_handler))
        .route("/", leptos_handler())
        .route("/artists", leptos_handler())
        .route("/artists/{artist_id}", leptos_handler())
        .route("/wanted", leptos_handler())
        .fallback_service(get_service(ServeDir::new(site_root)))
        .layer(
        TraceLayer::new_for_http()
            .on_request(|request: &axum::http::Request<_>, _span: &tracing::Span| {
                debug!(method = %request.method(), uri = %request.uri(), "HTTP request started");
            })
            .on_response(
                |response: &axum::http::Response<_>, latency: Duration, _span: &tracing::Span| {
                    let status = response.status().as_u16();
                    if status >= 500 {
                        error!(status, latency_ms = latency.as_millis(), "HTTP request failed");
                    } else if status >= 400 {
                        warn!(status, latency_ms = latency.as_millis(), "HTTP client error");
                    }
                },
            ),
    );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to 0.0.0.0:3000");

    info!(
        bind = "0.0.0.0:3000",
        database = %db_url_for_log,
        music_root = %music_root_for_log,
        default_quality = %default_quality_for_log,
        download_lyrics = download_lyrics_for_log,
        warning = %quality_warning_for_log,
        "Leptos SSR app started"
    );
    axum::serve(listener, app).await.expect("server error");
}

/// Execute a `ServerAction` against the real `AppState`.
///
/// This runs in the server process and has access to all binary-crate modules
/// (services, db, state). After mutating state it fires SSE so connected
/// clients refresh.
async fn dispatch_action_impl(
    state: AppState,
    action: yoink::shared::ServerAction,
) -> Result<(), String> {
    use chrono::Utc;
    use yoink::shared::ServerAction;

    match action {
        ServerAction::ToggleAlbumMonitor {
            album_id,
            monitored,
        } => {
            let mut album_to_queue = None;
            {
                let mut albums = state.monitored_albums.write().await;
                if let Some(album) = albums.iter_mut().find(|a| a.id == album_id) {
                    album.monitored = monitored;
                    services::update_wanted(album);
                    let _ = db::update_album_flags(
                        &state.db,
                        album.id,
                        album.monitored,
                        album.acquired,
                        album.wanted,
                    )
                    .await;
                    if album.monitored && !album.acquired {
                        album_to_queue = Some(album.clone());
                    }
                }
            }
            if let Some(album) = album_to_queue {
                services::enqueue_album_download(&state, &album).await;
            }
            state.notify_sse();
        }

        ServerAction::BulkMonitor {
            artist_id,
            monitored,
        } => {
            let mut to_queue = Vec::new();
            {
                let mut albums = state.monitored_albums.write().await;
                for album in albums.iter_mut().filter(|a| a.artist_id == artist_id) {
                    album.monitored = monitored;
                    services::update_wanted(album);
                    let _ = db::update_album_flags(
                        &state.db,
                        album.id,
                        album.monitored,
                        album.acquired,
                        album.wanted,
                    )
                    .await;
                    if album.monitored && !album.acquired {
                        to_queue.push(album.clone());
                    }
                }
            }
            for album in to_queue {
                services::enqueue_album_download(&state, &album).await;
            }
            state.notify_sse();
        }

        ServerAction::SyncArtistAlbums { artist_id } => {
            let _ = services::sync_artist_albums_from_hifi(&state, artist_id).await;
            state.notify_sse();
        }

        ServerAction::RemoveArtist { artist_id } => {
            let _ = db::delete_albums_by_artist(&state.db, artist_id).await;
            let _ = db::delete_artist(&state.db, artist_id).await;
            {
                let mut albums = state.monitored_albums.write().await;
                albums.retain(|a| a.artist_id != artist_id);
            }
            {
                let mut artists = state.monitored_artists.write().await;
                artists.retain(|a| a.id != artist_id);
            }
            info!(artist_id, "Removed artist and their albums");
            state.notify_sse();
        }

        ServerAction::AddArtist {
            id,
            name,
            picture,
            tidal_url,
        } => {
            {
                let mut artists = state.monitored_artists.write().await;
                if artists.iter().all(|a| a.id != id) {
                    let artist = yoink::shared::MonitoredArtist {
                        id,
                        name,
                        picture: picture.filter(|s| !s.is_empty()),
                        tidal_url: tidal_url.filter(|s| !s.is_empty()),
                        quality_profile: state.default_quality.clone(),
                        added_at: Utc::now(),
                    };
                    let _ = db::upsert_artist(&state.db, &artist).await;
                    artists.push(artist);
                }
            }
            let _ = services::sync_artist_albums_from_hifi(&state, id).await;
            state.notify_sse();
        }

        ServerAction::CancelDownload { job_id } => {
            let mut jobs = state.download_jobs.write().await;
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id)
                && matches!(job.status, yoink::shared::DownloadStatus::Queued)
            {
                job.status = yoink::shared::DownloadStatus::Failed;
                job.error = Some("Cancelled by user".to_string());
                job.updated_at = Utc::now();
                let _ = db::update_job(&state.db, job).await;
                info!(job_id, "Cancelled download job");
            }
            drop(jobs);
            state.notify_sse();
        }

        ServerAction::ClearCompleted => {
            let _ = db::delete_completed_jobs(&state.db).await;
            {
                let mut jobs = state.download_jobs.write().await;
                jobs.retain(|j| j.status != yoink::shared::DownloadStatus::Completed);
            }
            info!("Cleared completed download jobs");
            state.notify_sse();
        }

        ServerAction::RetryDownload { album_id } => {
            {
                let mut jobs = state.download_jobs.write().await;
                if let Some(job) = jobs.iter_mut().find(|j| {
                    j.album_id == album_id && j.status == yoink::shared::DownloadStatus::Failed
                }) {
                    let previous_quality = job.quality.clone();
                    job.status = yoink::shared::DownloadStatus::Queued;
                    job.quality = state.default_quality.clone();
                    job.error = None;
                    job.updated_at = Utc::now();
                    let _ = db::update_job(&state.db, job).await;
                    info!(
                        album_id,
                        job_id = job.id,
                        previous_quality = %previous_quality,
                        retry_quality = %job.quality,
                        "Retrying failed download job"
                    );
                    state.download_notify.notify_one();
                    state.notify_sse();
                    return Ok(());
                }
            }
            // No existing failed job — create a new one.
            let album = {
                let albums = state.monitored_albums.read().await;
                albums.iter().find(|a| a.id == album_id).cloned()
            };
            if let Some(album) = album {
                info!(album_id = album.id, title = %album.title, "Creating retry download job");
                services::enqueue_album_download(&state, &album).await;
            }
            state.notify_sse();
        }

        ServerAction::RemoveAlbumFiles { album_id } => {
            let album = {
                let albums = state.monitored_albums.read().await;
                albums.iter().find(|a| a.id == album_id).cloned()
            }
            .ok_or_else(|| format!("album {} not found", album_id))?;

            let removed = services::remove_downloaded_album_files(&state, &album).await?;

            let mut to_queue = None;
            {
                let mut albums = state.monitored_albums.write().await;
                if let Some(existing) = albums.iter_mut().find(|a| a.id == album_id) {
                    existing.acquired = false;
                    services::update_wanted(existing);
                    let _ = db::update_album_flags(
                        &state.db,
                        existing.id,
                        existing.monitored,
                        existing.acquired,
                        existing.wanted,
                    )
                    .await;
                    if existing.monitored {
                        to_queue = Some(existing.clone());
                    }
                }
            }

            let mut removed_completed_ids = Vec::new();
            {
                let mut jobs = state.download_jobs.write().await;
                jobs.retain(|j| {
                    let should_remove = j.album_id == album_id
                        && j.status == yoink::shared::DownloadStatus::Completed;
                    if should_remove {
                        removed_completed_ids.push(j.id);
                    }
                    !should_remove
                });
            }
            for job_id in removed_completed_ids {
                let _ = db::delete_job(&state.db, job_id).await;
            }

            if let Some(album) = to_queue {
                services::enqueue_album_download(&state, &album).await;
            }

            info!(album_id, removed, "Removed downloaded album files");
            state.notify_sse();
        }

        ServerAction::RetagLibrary => {
            let s = state.clone();
            tokio::spawn(async move {
                match services::retag_existing_files(&s).await {
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
        }

        ServerAction::ScanImportLibrary => {
            let s = state.clone();
            tokio::spawn(async move {
                match services::scan_and_import_library(&s).await {
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
        }
    }

    Ok(())
}
