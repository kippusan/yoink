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

use axum::routing::get;
use tower::layer::Layer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};

use crate::{
    app_config::AppConfig,
    config::QUALITY_WARNING,
    logging::init_logging,
    providers::{
        deezer::DeezerProvider, musicbrainz::MusicBrainzProvider, registry::ProviderRegistry,
        soulseek::SoulSeekSource, tidal::TidalProvider,
    },
    routes::build_router,
    services::{download_worker_loop, recompute_artist_match_suggestions, reconcile_library_files},
    state::AppState,
};

use yoink_app::{App, shell::shell};

/// Compute a relevance score (0.0–1.0) for a search result name against the query.
///
/// Uses a combination of:
/// - Jaro-Winkler similarity (good for short strings, prefix-biased)
/// - Exact / prefix / contains bonuses
///
/// Both `query` and `name` should be pre-lowercased.
fn search_relevance_score(query: &str, name: &str) -> f64 {
    if query == name {
        return 1.0;
    }

    let jw = strsim::jaro_winkler(query, name);

    // Bonus for exact prefix match (e.g. query "ivy" matches "ivy lab")
    let prefix_bonus = if name.starts_with(query) { 0.15 } else { 0.0 };

    // Bonus for containing the query as a substring
    let contains_bonus = if !name.starts_with(query) && name.contains(query) {
        0.05
    } else {
        0.0
    };

    (jw + prefix_bonus + contains_bonus).min(1.0)
}

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

    if app_config.musicbrainz_enabled {
        let mb = Arc::new(MusicBrainzProvider::new());
        registry.register_metadata(Arc::clone(&mb) as Arc<dyn providers::MetadataProvider>);
        info!("MusicBrainz metadata provider enabled");
    }

    if app_config.deezer_enabled {
        let deezer = Arc::new(DeezerProvider::new());
        registry.register_metadata(Arc::clone(&deezer) as Arc<dyn providers::MetadataProvider>);
        info!("Deezer metadata provider enabled");
    }

    if app_config.soulseek_enabled {
        let soulseek = Arc::new(SoulSeekSource::new(
            reqwest::Client::new(),
            app_config.slskd_base_url.clone(),
            app_config.slskd_username.clone(),
            app_config.slskd_password.clone(),
            app_config.slskd_downloads_dir.clone(),
        ));
        registry.register_download(Arc::clone(&soulseek) as Arc<dyn providers::DownloadSource>);
        info!("SoulSeek download source enabled");
    }

    let state = AppState::new(
        music_root,
        default_quality,
        app_config.download_lyrics,
        app_config.download_max_parallel_tracks,
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
            let query_lower = query.to_lowercase();

            // Collect results with a fuzzy relevance score.
            let mut scored: Vec<(f64, yoink_shared::SearchArtistResult)> = Vec::new();
            for (provider_id, artists) in all_results {
                for a in artists {
                    let image_url = a
                        .image_ref
                        .as_deref()
                        .map(|r| yoink_shared::provider_image_url(&provider_id, r, 160));
                    let name_lower = a.name.to_lowercase();
                    let score = search_relevance_score(&query_lower, &name_lower);
                    scored.push((
                        score,
                        yoink_shared::SearchArtistResult {
                            provider: provider_id.clone(),
                            external_id: a.external_id,
                            name: a.name,
                            image_url,
                            url: a.url,
                            disambiguation: a.disambiguation,
                            artist_type: a.artist_type,
                            country: a.country,
                            tags: a.tags,
                            popularity: a.popularity,
                        },
                    ));
                }
            }

            // Sort by descending relevance score.
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let results = scored.into_iter().map(|(_, r)| r).collect();
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
                            disambiguation: a.disambiguation,
                            artist_type: a.artist_type,
                            country: a.country,
                            tags: a.tags,
                            popularity: a.popularity,
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
                            for t in provider_tracks {
                                let local_track_id = if let Ok(Some(track_id)) =
                                    db::find_track_by_provider_link(
                                        &s.db,
                                        &link.provider,
                                        &t.external_id,
                                    )
                                    .await
                                {
                                    track_id
                                } else if let Some(ref isrc) = t.isrc {
                                    if let Ok(Some(track_id)) =
                                        db::find_track_by_album_isrc(&s.db, &album_id, isrc).await
                                    {
                                        track_id
                                    } else {
                                        db::uuid_to_string(&db::new_uuid())
                                    }
                                } else {
                                    db::find_track_by_album_position(
                                        &s.db,
                                        &album_id,
                                        t.disc_number.unwrap_or(1),
                                        t.track_number,
                                    )
                                    .await
                                    .ok()
                                    .flatten()
                                    .unwrap_or_else(|| db::uuid_to_string(&db::new_uuid()))
                                };

                                let explicit = t
                                    .extra
                                    .get("explicit")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);

                                let secs = t.duration_secs;
                                let mins = secs / 60;
                                let rem = secs % 60;
                                let track_info = yoink_shared::TrackInfo {
                                    id: local_track_id.clone(),
                                    title: t.title,
                                    version: t.version,
                                    disc_number: t.disc_number.unwrap_or(1),
                                    track_number: t.track_number,
                                    duration_secs: secs,
                                    duration_display: format!("{mins}:{rem:02}"),
                                    isrc: t.isrc,
                                    explicit,
                                };

                                let _ = db::upsert_track(&s.db, &track_info, &album_id).await;
                                let _ = db::upsert_track_provider_link(
                                    &s.db,
                                    &local_track_id,
                                    &link.provider,
                                    &t.external_id,
                                )
                                .await;
                            }

                            let persisted = db::load_tracks_for_album(&s.db, &album_id)
                                .await
                                .unwrap_or_default();
                            return Ok(persisted);
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

    let artist_suggestions_state = state.clone();
    let fetch_artist_match_suggestions_fn: yoink_shared::FetchArtistMatchSuggestionsFn =
        std::sync::Arc::new(move |artist_id: String| {
            let s = artist_suggestions_state.clone();
            Box::pin(async move {
                let artist_links = db::load_artist_provider_links(&s.db, &artist_id)
                    .await
                    .map_err(|e| format!("Failed to load artist provider links: {e}"))?;
                let linked_pairs: std::collections::HashSet<(String, String)> = artist_links
                    .iter()
                    .map(|l| (l.provider.clone(), l.external_id.clone()))
                    .collect();
                let link_lookup: std::collections::HashMap<
                    (String, String),
                    db::ArtistProviderLink,
                > = artist_links
                    .into_iter()
                    .map(|l| ((l.provider.clone(), l.external_id.clone()), l))
                    .collect();

                let suggestions = db::load_match_suggestions_for_scope(&s.db, "artist", &artist_id)
                    .await
                    .map_err(|e| format!("Failed to load artist match suggestions: {e}"))?;
                Ok(suggestions
                    .into_iter()
                    .map(|m| {
                        let left_linked = linked_pairs
                            .contains(&(m.left_provider.clone(), m.left_external_id.clone()));
                        let right_linked = linked_pairs
                            .contains(&(m.right_provider.clone(), m.right_external_id.clone()));

                        let use_right = if left_linked && !right_linked {
                            true
                        } else {
                            !right_linked || left_linked
                        };

                        let (provider, external_id, external_name, external_url, image_ref) =
                            if use_right {
                                (
                                    m.right_provider.clone(),
                                    m.right_external_id.clone(),
                                    m.external_name.clone(),
                                    m.external_url.clone(),
                                    m.image_ref.clone(),
                                )
                            } else if let Some(link) = link_lookup
                                .get(&(m.left_provider.clone(), m.left_external_id.clone()))
                            {
                                (
                                    m.left_provider.clone(),
                                    m.left_external_id.clone(),
                                    link.external_name.clone(),
                                    link.external_url.clone(),
                                    link.image_ref.clone(),
                                )
                            } else {
                                (
                                    m.left_provider.clone(),
                                    m.left_external_id.clone(),
                                    None,
                                    None,
                                    None,
                                )
                            };

                        let image_url = image_ref
                            .as_deref()
                            .map(|r| yoink_shared::provider_image_url(&provider, r, 160));
                        yoink_shared::MatchSuggestion {
                            id: m.id,
                            scope_type: m.scope_type,
                            scope_id: m.scope_id,
                            left_provider: m.left_provider,
                            left_external_id: m.left_external_id,
                            right_provider: provider,
                            right_external_id: external_id,
                            match_kind: m.match_kind,
                            confidence: m.confidence,
                            explanation: m.explanation,
                            external_name,
                            external_url,
                            image_url,
                            disambiguation: if use_right { m.disambiguation } else { None },
                            artist_type: if use_right { m.artist_type } else { None },
                            country: if use_right { m.country } else { None },
                            tags: if use_right { m.tags } else { Vec::new() },
                            popularity: if use_right { m.popularity } else { None },
                            status: m.status,
                        }
                    })
                    .collect())
            })
        });

    let album_suggestions_state = state.clone();
    let fetch_album_match_suggestions_fn: yoink_shared::FetchAlbumMatchSuggestionsFn =
        std::sync::Arc::new(move |album_id: String| {
            let s = album_suggestions_state.clone();
            Box::pin(async move {
                let album_links = db::load_album_provider_links(&s.db, &album_id)
                    .await
                    .map_err(|e| format!("Failed to load album provider links: {e}"))?;
                let linked_pairs: std::collections::HashSet<(String, String)> = album_links
                    .iter()
                    .map(|l| (l.provider.clone(), l.external_id.clone()))
                    .collect();
                let link_lookup: std::collections::HashMap<
                    (String, String),
                    db::AlbumProviderLink,
                > = album_links
                    .into_iter()
                    .map(|l| ((l.provider.clone(), l.external_id.clone()), l))
                    .collect();

                let suggestions = db::load_match_suggestions_for_scope(&s.db, "album", &album_id)
                    .await
                    .map_err(|e| format!("Failed to load album match suggestions: {e}"))?;
                Ok(suggestions
                    .into_iter()
                    .map(|m| {
                        let left_linked = linked_pairs
                            .contains(&(m.left_provider.clone(), m.left_external_id.clone()));
                        let right_linked = linked_pairs
                            .contains(&(m.right_provider.clone(), m.right_external_id.clone()));

                        let use_right = if left_linked && !right_linked {
                            true
                        } else {
                            !right_linked || left_linked
                        };

                        let (provider, external_id, external_name, external_url, image_ref) =
                            if use_right {
                                (
                                    m.right_provider.clone(),
                                    m.right_external_id.clone(),
                                    m.external_name.clone(),
                                    m.external_url.clone(),
                                    m.image_ref.clone(),
                                )
                            } else if let Some(link) = link_lookup
                                .get(&(m.left_provider.clone(), m.left_external_id.clone()))
                            {
                                (
                                    m.left_provider.clone(),
                                    m.left_external_id.clone(),
                                    link.external_title.clone(),
                                    link.external_url.clone(),
                                    link.cover_ref.clone(),
                                )
                            } else {
                                (
                                    m.left_provider.clone(),
                                    m.left_external_id.clone(),
                                    None,
                                    None,
                                    None,
                                )
                            };

                        let image_url = image_ref
                            .as_deref()
                            .map(|r| yoink_shared::provider_image_url(&provider, r, 160));
                        yoink_shared::MatchSuggestion {
                            id: m.id,
                            scope_type: m.scope_type,
                            scope_id: m.scope_id,
                            left_provider: m.left_provider,
                            left_external_id: m.left_external_id,
                            right_provider: provider,
                            right_external_id: external_id,
                            match_kind: m.match_kind,
                            confidence: m.confidence,
                            explanation: m.explanation,
                            external_name,
                            external_url,
                            image_url,
                            disambiguation: None,
                            artist_type: None,
                            country: None,
                            tags: Vec::new(),
                            popularity: None,
                            status: m.status,
                        }
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

    let preview_import_state = state.clone();
    let preview_import_fn: yoink_shared::PreviewImportFn =
        std::sync::Arc::new(move || {
            let s = preview_import_state.clone();
            Box::pin(async move { services::preview_import_library(&s).await })
        });

    let confirm_import_state = state.clone();
    let confirm_import_fn: yoink_shared::ConfirmImportFn =
        std::sync::Arc::new(move |items: Vec<yoink_shared::ImportConfirmation>| {
            let s = confirm_import_state.clone();
            Box::pin(async move { services::confirm_import_library(&s, items).await })
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
        fetch_artist_match_suggestions: fetch_artist_match_suggestions_fn,
        fetch_album_match_suggestions: fetch_album_match_suggestions_fn,
        dispatch_action: dispatch_action_fn,
        preview_import: preview_import_fn,
        confirm_import: confirm_import_fn,
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

    // Static assets (JS/WASM bundles, public files) are served under specific
    // paths. Everything else falls through to Leptos SSR so every client-side
    // route works on reload without manually registering it in Axum.
    let app = build_router(state)
        .route(
            "/leptos/{*fn_name}",
            get(server_fn_handler.clone()).post(server_fn_handler),
        )
        .nest_service("/pkg", ServeDir::new(format!("{}/pkg", site_root)))
        .nest_service("/favicon.ico", ServeDir::new(format!("{}/favicon.ico", site_root)))
        .nest_service("/yoink.svg", ServeDir::new(format!("{}/yoink.svg", site_root)))
        .fallback(leptos_handler())
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

/// Fetch artist bio from linked metadata providers in background.
fn spawn_fetch_artist_bio(state: &AppState, artist_id: String) {
    let s = state.clone();
    tokio::spawn(async move {
        // Load provider links for this artist
        let links = match db::load_artist_provider_links(&s.db, &artist_id).await {
            Ok(l) => l,
            Err(_) => return,
        };

        // Try each linked provider that supports bio fetching
        for link in &links {
            if let Some(provider) = s.registry.metadata_provider(&link.provider) {
                if let Some(bio) = provider.fetch_artist_bio(&link.external_id).await {
                    let _ = db::update_artist_bio(&s.db, &artist_id, Some(&bio)).await;
                    // Update in-memory state
                    {
                        let mut artists = s.monitored_artists.write().await;
                        if let Some(a) = artists.iter_mut().find(|a| a.id == artist_id) {
                            a.bio = Some(bio);
                        }
                    }
                    s.notify_sse();
                    return;
                }
            }
        }
    });
}

fn spawn_recompute_artist_match_suggestions(state: &AppState, artist_id: String) {
    let s = state.clone();
    tokio::spawn(async move {
        if let Err(err) = recompute_artist_match_suggestions(&s, &artist_id).await {
            warn!(artist_id = %artist_id, error = %err, "Background match recompute failed");
        }
        s.notify_sse();
    });
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
            // Fetch bio if we don't have one yet
            {
                let artists = state.monitored_artists.read().await;
                let has_bio = artists
                    .iter()
                    .find(|a| a.id == artist_id)
                    .map(|a| a.bio.is_some())
                    .unwrap_or(false);
                if !has_bio {
                    spawn_fetch_artist_bio(&state, artist_id.clone());
                }
            }
            spawn_recompute_artist_match_suggestions(&state, artist_id.clone());
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
                    bio: None,
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
            spawn_fetch_artist_bio(&state, artist_id.clone());
            spawn_recompute_artist_match_suggestions(&state, artist_id.clone());
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
            spawn_recompute_artist_match_suggestions(&state, link.artist_id.clone());
            state.notify_sse();
        }

        ServerAction::UnlinkArtistProvider {
            artist_id,
            provider,
            external_id,
        } => {
            let _ = db::delete_artist_provider_link(&state.db, &artist_id, &provider, &external_id)
                .await;
            spawn_recompute_artist_match_suggestions(&state, artist_id.clone());
            state.notify_sse();
        }

        ServerAction::AcceptMatchSuggestion { suggestion_id } => {
            let suggestion = db::load_match_suggestion_by_id(&state.db, &suggestion_id)
                .await
                .map_err(|e| format!("failed to load match suggestion: {e}"))?
                .ok_or_else(|| "match suggestion not found".to_string())?;

            match suggestion.scope_type.as_str() {
                "album" => {
                    let album_links =
                        db::load_album_provider_links(&state.db, &suggestion.scope_id)
                            .await
                            .map_err(|e| format!("failed loading album links: {e}"))?;
                    let linked: std::collections::HashSet<(String, String)> = album_links
                        .iter()
                        .map(|l| (l.provider.clone(), l.external_id.clone()))
                        .collect();
                    let left_linked = linked.contains(&(
                        suggestion.left_provider.clone(),
                        suggestion.left_external_id.clone(),
                    ));
                    let right_linked = linked.contains(&(
                        suggestion.right_provider.clone(),
                        suggestion.right_external_id.clone(),
                    ));
                    let (target_provider, target_external_id, target_url) =
                        if left_linked && !right_linked {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        } else if right_linked && !left_linked {
                            (
                                suggestion.left_provider.clone(),
                                suggestion.left_external_id.clone(),
                                None, // external_url on the suggestion is for the right side
                            )
                        } else {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        };

                    let existing = db::find_album_by_provider_link(
                        &state.db,
                        &target_provider,
                        &target_external_id,
                    )
                    .await
                    .map_err(|e| format!("failed checking existing album link: {e}"))?;

                    if let Some(existing_album_id) = existing
                        && existing_album_id != suggestion.scope_id
                    {
                        return Err(
                            "Cannot accept: provider album is already linked to another local album"
                                .to_string(),
                        );
                    }

                    let link = db::AlbumProviderLink {
                        id: db::uuid_to_string(&db::new_uuid()),
                        album_id: suggestion.scope_id.clone(),
                        provider: target_provider,
                        external_id: target_external_id,
                        external_url: target_url,
                        external_title: suggestion.external_name.clone(),
                        cover_ref: None,
                    };
                    let _ = db::upsert_album_provider_link(&state.db, &link).await;
                }
                "artist" => {
                    let artist_links =
                        db::load_artist_provider_links(&state.db, &suggestion.scope_id)
                            .await
                            .map_err(|e| format!("failed loading artist links: {e}"))?;
                    let linked: std::collections::HashSet<(String, String)> = artist_links
                        .iter()
                        .map(|l| (l.provider.clone(), l.external_id.clone()))
                        .collect();
                    let left_linked = linked.contains(&(
                        suggestion.left_provider.clone(),
                        suggestion.left_external_id.clone(),
                    ));
                    let right_linked = linked.contains(&(
                        suggestion.right_provider.clone(),
                        suggestion.right_external_id.clone(),
                    ));
                    let (target_provider, target_external_id, target_url) =
                        if left_linked && !right_linked {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        } else if right_linked && !left_linked {
                            (
                                suggestion.left_provider.clone(),
                                suggestion.left_external_id.clone(),
                                None,
                            )
                        } else {
                            (
                                suggestion.right_provider.clone(),
                                suggestion.right_external_id.clone(),
                                suggestion.external_url.clone(),
                            )
                        };

                    let existing = db::find_artist_by_provider_link(
                        &state.db,
                        &target_provider,
                        &target_external_id,
                    )
                    .await
                    .map_err(|e| format!("failed checking existing artist link: {e}"))?;

                    if let Some(existing_artist_id) = existing
                        && existing_artist_id != suggestion.scope_id
                    {
                        return Err(
                            "Cannot accept: provider artist is already linked to another local artist"
                                .to_string(),
                        );
                    }

                    let link = db::ArtistProviderLink {
                        id: db::uuid_to_string(&db::new_uuid()),
                        artist_id: suggestion.scope_id.clone(),
                        provider: target_provider,
                        external_id: target_external_id,
                        external_url: target_url,
                        external_name: suggestion.external_name.clone(),
                        image_ref: None,
                    };
                    let _ = db::upsert_artist_provider_link(&state.db, &link).await;

                    // Pull albums from the newly linked provider, then recompute matches.
                    let _ = services::sync_artist_albums(&state, &suggestion.scope_id).await;
                    spawn_recompute_artist_match_suggestions(&state, suggestion.scope_id.clone());
                }
                _ => return Err("unknown suggestion scope type".to_string()),
            }

            let _ = db::set_match_suggestion_status(&state.db, &suggestion_id, "accepted").await;

            if suggestion.scope_type == "album" {
                // Keep artist-level suggestions fresh after album linking decisions.
                let artist_id = {
                    let albums = state.monitored_albums.read().await;
                    albums
                        .iter()
                        .find(|a| a.id == suggestion.scope_id)
                        .map(|a| a.artist_id.clone())
                };
                if let Some(artist_id) = artist_id {
                    spawn_recompute_artist_match_suggestions(&state, artist_id);
                }
            }

            state.notify_sse();
        }

        ServerAction::DismissMatchSuggestion { suggestion_id } => {
            let scope = db::load_match_suggestion_by_id(&state.db, &suggestion_id)
                .await
                .ok()
                .flatten();
            let _ = db::set_match_suggestion_status(&state.db, &suggestion_id, "dismissed").await;

            if let Some(suggestion) = scope
                && suggestion.scope_type == "album"
            {
                let artist_id = {
                    let albums = state.monitored_albums.read().await;
                    albums
                        .iter()
                        .find(|a| a.id == suggestion.scope_id)
                        .map(|a| a.artist_id.clone())
                };
                if let Some(artist_id) = artist_id {
                    spawn_recompute_artist_match_suggestions(&state, artist_id);
                }
            }
            state.notify_sse();
        }

        ServerAction::RefreshMatchSuggestions { artist_id } => {
            let _ = recompute_artist_match_suggestions(&state, &artist_id).await;
            state.notify_sse();
        }

        ServerAction::MergeAlbums {
            target_album_id,
            source_album_id,
            result_title,
            result_cover_url,
        } => {
            services::merge_albums(
                &state,
                &target_album_id,
                &source_album_id,
                result_title.as_deref(),
                result_cover_url.as_deref(),
            )
            .await?;

            let artist_id = {
                let albums = state.monitored_albums.read().await;
                albums
                    .iter()
                    .find(|a| a.id == target_album_id)
                    .map(|a| a.artist_id.clone())
            };
            if let Some(artist_id) = artist_id {
                spawn_recompute_artist_match_suggestions(&state, artist_id);
            }
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

        ServerAction::ConfirmImport { items } => {
            let summary = services::confirm_import_library(&state, items).await?;
            info!(
                total = summary.total_selected,
                imported = summary.imported,
                artists_added = summary.artists_added,
                failed = summary.failed,
                "Confirmed import completed"
            );
            if !summary.errors.is_empty() {
                return Err(format!(
                    "Imported {}/{} albums. Errors: {}",
                    summary.imported,
                    summary.total_selected,
                    summary.errors.join("; ")
                ));
            }
        }
    }

    Ok(())
}
