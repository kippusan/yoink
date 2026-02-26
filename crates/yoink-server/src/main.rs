mod app_config;
mod config;
mod db;
mod logging;
mod models;
mod providers;
mod routes;
mod services;
mod state;
mod ui;

use std::{sync::Arc, time::Duration};

use axum::routing::{get, get_service};
use tower::layer::Layer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};

use crate::{
    app_config::AppConfig,
    config::QUALITY_WARNING,
    logging::init_logging,
    providers::{registry::ProviderRegistry, tidal::TidalProvider},
    routes::build_router,
    services::{download_worker_loop, reconcile_library_files},
    state::AppState,
};

use yoink_app::{App, shell::shell};

#[tokio::main]
async fn main() {
    let app_config = AppConfig::from_env().unwrap_or_else(|err| {
        panic!("Failed to parse configuration from environment: {err}");
    });

    init_logging(&app_config.log_format);

    // Initialise the Leptos/Tokio executor so SSR rendering can spawn futures.
    // generate_route_list calls any_spawner::Executor::init_tokio() internally.
    let _leptos_routes = leptos_axum::generate_route_list(App);

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

    // ── Build provider registry ─────────────────────────────────
    let mut registry = ProviderRegistry::new();

    if app_config.tidal_enabled {
        let tidal_base_url = app_config.resolved_tidal_base_url();
        let tidal = Arc::new(TidalProvider::new(
            reqwest::Client::new(),
            Some(tidal_base_url),
        ));
        registry.register_metadata(Arc::clone(&tidal) as Arc<dyn providers::MetadataProvider>);
        registry.register_download(Arc::clone(&tidal) as Arc<dyn providers::DownloadSource>);
        registry.set_tidal(Arc::clone(&tidal));
        info!("Tidal provider enabled");
    }

    let state = AppState::new(
        music_root,
        default_quality,
        app_config.download_lyrics,
        &db_url,
        registry,
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
    let search_fn: yoink_shared::SearchArtistsFn = std::sync::Arc::new(move |query: String| {
        let s = search_state.clone();
        Box::pin(async move {
            let all_results = s.registry.search_artists_all(&query).await;
            let mut results = Vec::new();
            for (provider_id, artists) in all_results {
                for a in artists {
                    let image_url = a
                        .image_ref
                        .as_deref()
                        .map(|r| yoink_shared::provider_image_url(&provider_id, r, 160));
                    results.push(yoink_shared::SearchArtistResult {
                        provider: provider_id.clone(),
                        external_id: a.external_id,
                        name: a.name,
                        image_url,
                        url: a.url,
                    });
                }
            }
            Ok(results)
        })
    });

    let scoped_search_state = state.clone();
    let search_scoped_fn: yoink_shared::SearchArtistsScopedFn =
        std::sync::Arc::new(move |provider_id: String, query: String| {
            let s = scoped_search_state.clone();
            Box::pin(async move {
                let artists = s
                    .registry
                    .search_artists(&provider_id, &query)
                    .await
                    .map_err(|e| e.to_string())?;
                let results = artists
                    .into_iter()
                    .map(|a| {
                        let image_url = a
                            .image_ref
                            .as_deref()
                            .map(|r| yoink_shared::provider_image_url(&provider_id, r, 160));
                        yoink_shared::SearchArtistResult {
                            provider: provider_id.clone(),
                            external_id: a.external_id,
                            name: a.name,
                            image_url,
                            url: a.url,
                        }
                    })
                    .collect();
                Ok(results)
            })
        });

    let list_providers_registry = state.registry.clone();
    let list_providers_fn: yoink_shared::ListProvidersFn =
        std::sync::Arc::new(move || list_providers_registry.metadata_provider_ids());

    let tracks_state = state.clone();
    let fetch_tracks_fn: yoink_shared::FetchTracksFn =
        std::sync::Arc::new(move |album_id: String| {
            let s = tracks_state.clone();
            Box::pin(async move {
                // First try to load from local DB
                let tracks = db::load_tracks_for_album(&s.db, &album_id)
                    .await
                    .map_err(|e| format!("Failed to load tracks: {e}"))?;

                if !tracks.is_empty() {
                    return Ok(tracks);
                }

                // Fallback: try to fetch from any linked metadata provider
                let links = db::load_album_provider_links(&s.db, &album_id)
                    .await
                    .map_err(|e| format!("Failed to load album links: {e}"))?;

                for link in &links {
                    let Some(provider) = s.registry.metadata_provider(&link.provider) else {
                        continue;
                    };

                    match provider.fetch_tracks(&link.external_id).await {
                        Ok((provider_tracks, _album_extra)) => {
                            return Ok(provider_tracks
                                .into_iter()
                                .map(|t| {
                                    let secs = t.duration_secs;
                                    let mins = secs / 60;
                                    let rem = secs % 60;
                                    yoink_shared::TrackInfo {
                                        id: db::uuid_to_string(&db::new_uuid()),
                                        title: t.title,
                                        version: t.version,
                                        disc_number: t.disc_number.unwrap_or(1),
                                        track_number: t.track_number,
                                        duration_secs: secs,
                                        duration_display: format!("{mins}:{rem:02}"),
                                        isrc: t.isrc,
                                    }
                                })
                                .collect());
                        }
                        Err(_) => continue,
                    }
                }

                Ok(Vec::new())
            })
        });

    let links_state = state.clone();
    let fetch_artist_links_fn: yoink_shared::FetchArtistLinksFn =
        std::sync::Arc::new(move |artist_id: String| {
            let s = links_state.clone();
            Box::pin(async move {
                let links = db::load_artist_provider_links(&s.db, &artist_id)
                    .await
                    .map_err(|e| format!("Failed to load provider links: {e}"))?;
                Ok(links
                    .into_iter()
                    .map(|l| yoink_shared::ProviderLink {
                        provider: l.provider,
                        external_id: l.external_id,
                        external_url: l.external_url,
                        external_name: l.external_name,
                    })
                    .collect())
            })
        });

    let album_links_state = state.clone();
    let fetch_album_links_fn: yoink_shared::FetchAlbumLinksFn =
        std::sync::Arc::new(move |album_id: String| {
            let s = album_links_state.clone();
            Box::pin(async move {
                let links = db::load_album_provider_links(&s.db, &album_id)
                    .await
                    .map_err(|e| format!("Failed to load album provider links: {e}"))?;
                Ok(links
                    .into_iter()
                    .map(|l| yoink_shared::ProviderLink {
                        provider: l.provider,
                        external_id: l.external_id,
                        external_url: l.external_url,
                        external_name: l.external_title,
                    })
                    .collect())
            })
        });

    let action_state = state.clone();
    let dispatch_action_fn: yoink_shared::DispatchActionFn =
        std::sync::Arc::new(move |action: yoink_shared::ServerAction| {
            let s = action_state.clone();
            Box::pin(async move { dispatch_action_impl(s, action).await })
        });

    let server_ctx = yoink_shared::ServerContext {
        monitored_artists: state.monitored_artists.clone(),
        monitored_albums: state.monitored_albums.clone(),
        download_jobs: state.download_jobs.clone(),
        search_artists: search_fn,
        search_artists_scoped: search_scoped_fn,
        list_providers: list_providers_fn,
        fetch_tracks: fetch_tracks_fn,
        fetch_artist_links: fetch_artist_links_fn,
        fetch_album_links: fetch_album_links_fn,
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
    let leptos_handler = || {
        let ctx = provide_server_ctx.clone();
        get(leptos_axum::render_app_to_stream_with_context(ctx, shell))
    };

    let app = build_router(state)
        .route(
            "/leptos/{*fn_name}",
            get(server_fn_handler.clone()).post(server_fn_handler),
        )
        .route("/", leptos_handler())
        .route("/artists", leptos_handler())
        .route("/artists/{artist_id}", leptos_handler())
        .route("/artists/{artist_id}/albums/{album_id}", leptos_handler())
        .route("/wanted", leptos_handler())
        .fallback_service(get_service(ServeDir::new(site_root)))
        .layer(
            TraceLayer::new_for_http()
                .on_request(
                    |request: &axum::http::Request<_>, _span: &tracing::Span| {
                        debug!(method = %request.method(), uri = %request.uri(), "HTTP request started");
                    },
                )
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: Duration,
                     _span: &tracing::Span| {
                        let status = response.status().as_u16();
                        if status >= 500 {
                            error!(
                                status,
                                latency_ms = latency.as_millis(),
                                "HTTP request failed"
                            );
                        } else if status >= 400 {
                            warn!(
                                status,
                                latency_ms = latency.as_millis(),
                                "HTTP client error"
                            );
                        }
                    },
                ),
        );

    // NormalizePath strips trailing slashes so `/artists/` matches `/artists`.
    let app = NormalizePathLayer::trim_trailing_slash().layer(app);

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
    axum::serve(
        listener,
        axum::ServiceExt::<axum::http::Request<axum::body::Body>>::into_make_service(app),
    )
    .await
    .expect("server error");
}

/// Execute a `ServerAction` against the real `AppState`.
async fn dispatch_action_impl(
    state: AppState,
    action: yoink_shared::ServerAction,
) -> Result<(), String> {
    use chrono::Utc;
    use yoink_shared::ServerAction;

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
                        &album.id,
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
                        &album.id,
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
            let _ = services::sync_artist_albums(&state, &artist_id).await;
            state.notify_sse();
        }

        ServerAction::RemoveArtist {
            artist_id,
            remove_files,
        } => {
            if remove_files {
                let acquired: Vec<_> = {
                    let albums = state.monitored_albums.read().await;
                    albums
                        .iter()
                        .filter(|a| a.artist_id == artist_id && a.acquired)
                        .cloned()
                        .collect()
                };
                for album in &acquired {
                    if let Err(e) = services::remove_downloaded_album_files(&state, album).await {
                        warn!(
                            album_id = %album.id,
                            error = %e,
                            "Failed to remove files for album while removing artist"
                        );
                    }
                }
            }
            let _ = db::delete_albums_by_artist(&state.db, &artist_id).await;
            let _ = db::delete_artist(&state.db, &artist_id).await;
            {
                let mut albums = state.monitored_albums.write().await;
                albums.retain(|a| a.artist_id != artist_id);
            }
            {
                let mut artists = state.monitored_artists.write().await;
                artists.retain(|a| a.id != artist_id);
            }
            info!(%artist_id, remove_files, "Removed artist and their albums");
            state.notify_sse();
        }

        ServerAction::AddArtist {
            name,
            provider,
            external_id,
            image_url,
            external_url,
        } => {
            // Check if we already have this provider link
            let existing_artist_id =
                db::find_artist_by_provider_link(&state.db, &provider, &external_id)
                    .await
                    .ok()
                    .flatten();

            let artist_id = if let Some(id) = existing_artist_id {
                id
            } else {
                // Create new local artist
                let new_id = db::uuid_to_string(&db::new_uuid());
                let artist = yoink_shared::MonitoredArtist {
                    id: new_id.clone(),
                    name: name.clone(),
                    image_url: image_url.clone(),
                    added_at: Utc::now(),
                };
                let _ = db::upsert_artist(&state.db, &artist).await;
                {
                    let mut artists = state.monitored_artists.write().await;
                    artists.push(artist);
                }

                // Create the provider link
                let link = db::ArtistProviderLink {
                    id: db::uuid_to_string(&db::new_uuid()),
                    artist_id: new_id.clone(),
                    provider: provider.clone(),
                    external_id: external_id.clone(),
                    external_url: external_url.clone(),
                    external_name: Some(name),
                    image_ref: None,
                };
                let _ = db::upsert_artist_provider_link(&state.db, &link).await;

                new_id
            };

            let _ = services::sync_artist_albums(&state, &artist_id).await;
            state.notify_sse();
        }

        ServerAction::LinkArtistProvider {
            artist_id,
            provider,
            external_id,
            external_url,
            external_name,
            image_ref,
        } => {
            let link = db::ArtistProviderLink {
                id: db::uuid_to_string(&db::new_uuid()),
                artist_id,
                provider,
                external_id,
                external_url,
                external_name,
                image_ref,
            };
            let _ = db::upsert_artist_provider_link(&state.db, &link).await;
            state.notify_sse();
        }

        ServerAction::UnlinkArtistProvider {
            artist_id,
            provider,
            external_id,
        } => {
            let _ =
                db::delete_artist_provider_link(&state.db, &artist_id, &provider, &external_id)
                    .await;
            state.notify_sse();
        }

        ServerAction::CancelDownload { job_id } => {
            let mut jobs = state.download_jobs.write().await;
            if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id)
                && matches!(job.status, yoink_shared::DownloadStatus::Queued)
            {
                job.status = yoink_shared::DownloadStatus::Failed;
                job.error = Some("Cancelled by user".to_string());
                job.updated_at = Utc::now();
                let _ = db::update_job(&state.db, job).await;
                info!(%job_id, "Cancelled download job");
            }
            drop(jobs);
            state.notify_sse();
        }

        ServerAction::ClearCompleted => {
            let _ = db::delete_completed_jobs(&state.db).await;
            {
                let mut jobs = state.download_jobs.write().await;
                jobs.retain(|j| j.status != yoink_shared::DownloadStatus::Completed);
            }
            info!("Cleared completed download jobs");
            state.notify_sse();
        }

        ServerAction::RetryDownload { album_id } => {
            {
                let mut jobs = state.download_jobs.write().await;
                if let Some(job) = jobs.iter_mut().find(|j| {
                    j.album_id == album_id && j.status == yoink_shared::DownloadStatus::Failed
                }) {
                    let previous_quality = job.quality.clone();
                    job.status = yoink_shared::DownloadStatus::Queued;
                    job.quality = state.default_quality.clone();
                    job.error = None;
                    job.updated_at = Utc::now();
                    let _ = db::update_job(&state.db, job).await;
                    info!(
                        %album_id,
                        job_id = %job.id,
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
                info!(album_id = %album.id, title = %album.title, "Creating retry download job");
                services::enqueue_album_download(&state, &album).await;
            }
            state.notify_sse();
        }

        ServerAction::RemoveAlbumFiles {
            album_id,
            unmonitor,
        } => {
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
                    if unmonitor {
                        existing.monitored = false;
                    }
                    services::update_wanted(existing);
                    let _ = db::update_album_flags(
                        &state.db,
                        &existing.id,
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
                        && j.status == yoink_shared::DownloadStatus::Completed;
                    if should_remove {
                        removed_completed_ids.push(j.id.clone());
                    }
                    !should_remove
                });
            }
            for job_id in removed_completed_ids {
                let _ = db::delete_job(&state.db, &job_id).await;
            }

            if let Some(album) = to_queue {
                services::enqueue_album_download(&state, &album).await;
            }

            info!(
                %album_id,
                removed, unmonitor, "Removed downloaded album files"
            );
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
