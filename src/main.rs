mod config;
mod db;
mod logging;
mod models;
mod routes;
mod services;
mod state;
mod ui;

use std::{path::PathBuf, time::Duration};

use axum::routing::get_service;
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

#[tokio::main]
async fn main() {
    init_logging();

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

    let app = build_router(state)
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
