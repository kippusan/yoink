mod config;
mod db;
mod logging;
mod models;
mod routes;
mod services;
mod state;
mod ui;

use std::{path::PathBuf, time::Duration};

use axum::routing::{get, get_service};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};

use crate::{
    config::{DEFAULT_QUALITY, QUALITY_WARNING},
    logging::init_logging,
    routes::build_router,
    services::{download_worker_loop, reconcile_library_files},
    state::AppState,
};

use yoink::app::{shell::shell, App};

#[tokio::main]
async fn main() {
    init_logging();

    // Initialise the Leptos/Tokio executor so SSR rendering can spawn futures.
    // generate_route_list calls any_spawner::Executor::init_tokio() internally.
    let _leptos_routes = leptos_axum::generate_route_list(App);

    let manual_hifi_base_url = std::env::var("HIFI_API_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim_end_matches('/').to_string())
        .or(Some("http://127.0.0.1:8000".to_string()));
    let music_root = std::env::var("MUSIC_ROOT")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./music"));
    let default_quality = std::env::var("DEFAULT_QUALITY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_QUALITY.to_string());
    let default_quality_for_log = default_quality.clone();
    let music_root_for_log = music_root.display().to_string();

    let db_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "sqlite:./yoink.db?mode=rwc".to_string());
    let db_url_for_log = db_url.clone();

    let state = AppState::new(manual_hifi_base_url, music_root, default_quality, &db_url).await;
    if let Err(err) = reconcile_library_files(&state).await {
        warn!(error = %err, "Initial library reconciliation failed");
    }
    let site_root = std::env::var("LEPTOS_SITE_ROOT").unwrap_or_else(|_| "target/site".to_string());

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

    let server_ctx = yoink::shared::ServerContext {
        monitored_artists: state.monitored_artists.clone(),
        monitored_albums: state.monitored_albums.clone(),
        download_jobs: state.download_jobs.clone(),
        search_artists: search_fn,
        fetch_tracks: fetch_tracks_fn,
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
                    } else {
                        info!(status, latency_ms = latency.as_millis(), "HTTP request completed");
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
        warning = %QUALITY_WARNING,
        "Leptos SSR app started"
    );
    axum::serve(listener, app).await.expect("server error");
}
