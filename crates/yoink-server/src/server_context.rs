use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::{actions::dispatch_action_impl, db, services, state::AppState};

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

    let prefix_bonus = if name.starts_with(query) { 0.15 } else { 0.0 };

    let contains_bonus = if !name.starts_with(query) && name.contains(query) {
        0.05
    } else {
        0.0
    };

    (jw + prefix_bonus + contains_bonus).min(1.0)
}

/// Build the `ServerContext` that wires Leptos server functions to the real `AppState`.
pub(crate) fn build_server_context(state: &AppState) -> yoink_shared::ServerContext {
    let search_fn = build_search_fn(state);
    let search_scoped_fn = build_search_scoped_fn(state);
    let list_providers_fn = build_list_providers_fn(state);
    let fetch_tracks_fn = build_fetch_tracks_fn(state);
    let fetch_artist_links_fn = build_fetch_artist_links_fn(state);
    let fetch_album_links_fn = build_fetch_album_links_fn(state);
    let fetch_artist_match_suggestions_fn = build_fetch_artist_match_suggestions_fn(state);
    let fetch_album_match_suggestions_fn = build_fetch_album_match_suggestions_fn(state);
    let dispatch_action_fn = build_dispatch_action_fn(state);
    let preview_import_fn = build_preview_import_fn(state);
    let confirm_import_fn = build_confirm_import_fn(state);

    yoink_shared::ServerContext {
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
    }
}

// ── Individual callback builders ────────────────────────────────────

fn build_search_fn(state: &AppState) -> yoink_shared::SearchArtistsFn {
    let s = state.clone();
    std::sync::Arc::new(move |query: String| {
        let s = s.clone();
        Box::pin(async move {
            let all_results = s.registry.search_artists_all(&query).await;
            let query_lower = query.to_lowercase();

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

            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let results = scored.into_iter().map(|(_, r)| r).collect();
            Ok(results)
        })
    })
}

fn build_search_scoped_fn(state: &AppState) -> yoink_shared::SearchArtistsScopedFn {
    let s = state.clone();
    std::sync::Arc::new(move |provider_id: String, query: String| {
        let s = s.clone();
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
    })
}

fn build_list_providers_fn(state: &AppState) -> yoink_shared::ListProvidersFn {
    let registry = state.registry.clone();
    std::sync::Arc::new(move || registry.metadata_provider_ids())
}

fn build_fetch_tracks_fn(state: &AppState) -> yoink_shared::FetchTracksFn {
    let s = state.clone();
    std::sync::Arc::new(move |album_id: Uuid| {
        let s = s.clone();
        Box::pin(async move {
            // First try to load from local DB
            let tracks = db::load_tracks_for_album(&s.db, album_id)
                .await
                .map_err(|e| format!("Failed to load tracks: {e}"))?;

            let needs_artist_backfill =
                !tracks.is_empty() && tracks.iter().all(|t| t.track_artist.is_none());
            if !tracks.is_empty() && !needs_artist_backfill {
                return Ok(tracks);
            }

            // Fallback: try to fetch from any linked metadata provider
            let links = db::load_album_provider_links(&s.db, album_id)
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
                                    db::find_track_by_album_isrc(&s.db, album_id, isrc).await
                                {
                                    track_id
                                } else {
                                    Uuid::now_v7()
                                }
                            } else {
                                db::find_track_by_album_position(
                                    &s.db,
                                    album_id,
                                    t.disc_number.unwrap_or(1),
                                    t.track_number,
                                )
                                .await
                                .ok()
                                .flatten()
                                .unwrap_or_else(Uuid::now_v7)
                            };

                            let secs = t.duration_secs;
                            let mins = secs / 60;
                            let rem = secs % 60;
                            let track_info = yoink_shared::TrackInfo {
                                id: local_track_id,
                                title: t.title,
                                version: t.version,
                                disc_number: t.disc_number.unwrap_or(1),
                                track_number: t.track_number,
                                duration_secs: secs,
                                duration_display: format!("{mins}:{rem:02}"),
                                isrc: t.isrc,
                                explicit: t.explicit,
                                track_artist: t.artists,
                                file_path: None,
                            };

                            let _ = db::upsert_track(&s.db, &track_info, album_id).await;
                            let _ = db::upsert_track_provider_link(
                                &s.db,
                                local_track_id,
                                &link.provider,
                                &t.external_id,
                            )
                            .await;
                        }

                        let persisted = db::load_tracks_for_album(&s.db, album_id)
                            .await
                            .unwrap_or_default();
                        return Ok(persisted);
                    }
                    Err(_) => continue,
                }
            }

            Ok(Vec::new())
        })
    })
}

fn build_fetch_artist_links_fn(state: &AppState) -> yoink_shared::FetchArtistLinksFn {
    let s = state.clone();
    std::sync::Arc::new(move |artist_id: Uuid| {
        let s = s.clone();
        Box::pin(async move {
            let links = db::load_artist_provider_links(&s.db, artist_id)
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
    })
}

fn build_fetch_album_links_fn(state: &AppState) -> yoink_shared::FetchAlbumLinksFn {
    let s = state.clone();
    std::sync::Arc::new(move |album_id: Uuid| {
        let s = s.clone();
        Box::pin(async move {
            let links = db::load_album_provider_links(&s.db, album_id)
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
    })
}

fn build_fetch_artist_match_suggestions_fn(
    state: &AppState,
) -> yoink_shared::FetchArtistMatchSuggestionsFn {
    let s = state.clone();
    std::sync::Arc::new(move |artist_id: Uuid| {
        let s = s.clone();
        Box::pin(async move {
            let artist_links = db::load_artist_provider_links(&s.db, artist_id)
                .await
                .map_err(|e| format!("Failed to load artist provider links: {e}"))?;
            let linked_pairs: HashSet<(String, String)> = artist_links
                .iter()
                .map(|l| (l.provider.clone(), l.external_id.clone()))
                .collect();
            let link_lookup: HashMap<(String, String), db::ArtistProviderLink> = artist_links
                .into_iter()
                .map(|l| ((l.provider.clone(), l.external_id.clone()), l))
                .collect();

            let suggestions = db::load_match_suggestions_for_scope(&s.db, "artist", artist_id)
                .await
                .map_err(|e| format!("Failed to load artist match suggestions: {e}"))?;
            Ok(suggestions
                .into_iter()
                .map(|m| map_artist_suggestion(m, &linked_pairs, &link_lookup))
                .collect())
        })
    })
}

fn build_fetch_album_match_suggestions_fn(
    state: &AppState,
) -> yoink_shared::FetchAlbumMatchSuggestionsFn {
    let s = state.clone();
    std::sync::Arc::new(move |album_id: Uuid| {
        let s = s.clone();
        Box::pin(async move {
            let album_links = db::load_album_provider_links(&s.db, album_id)
                .await
                .map_err(|e| format!("Failed to load album provider links: {e}"))?;
            let linked_pairs: HashSet<(String, String)> = album_links
                .iter()
                .map(|l| (l.provider.clone(), l.external_id.clone()))
                .collect();
            let link_lookup: HashMap<(String, String), db::AlbumProviderLink> = album_links
                .into_iter()
                .map(|l| ((l.provider.clone(), l.external_id.clone()), l))
                .collect();

            let suggestions = db::load_match_suggestions_for_scope(&s.db, "album", album_id)
                .await
                .map_err(|e| format!("Failed to load album match suggestions: {e}"))?;
            Ok(suggestions
                .into_iter()
                .map(|m| map_album_suggestion(m, &linked_pairs, &link_lookup))
                .collect())
        })
    })
}

fn build_dispatch_action_fn(state: &AppState) -> yoink_shared::DispatchActionFn {
    let s = state.clone();
    std::sync::Arc::new(move |action: yoink_shared::ServerAction| {
        let s = s.clone();
        Box::pin(async move { dispatch_action_impl(s, action).await })
    })
}

fn build_preview_import_fn(state: &AppState) -> yoink_shared::PreviewImportFn {
    let s = state.clone();
    std::sync::Arc::new(move || {
        let s = s.clone();
        Box::pin(async move { services::preview_import_library(&s).await })
    })
}

fn build_confirm_import_fn(state: &AppState) -> yoink_shared::ConfirmImportFn {
    let s = state.clone();
    std::sync::Arc::new(move |items: Vec<yoink_shared::ImportConfirmation>| {
        let s = s.clone();
        Box::pin(async move { services::confirm_import_library(&s, items).await })
    })
}

// ── Suggestion mapping helpers ──────────────────────────────────────

fn resolve_suggestion_side(
    m: &db::MatchSuggestion,
    linked_pairs: &HashSet<(String, String)>,
) -> bool {
    let left_linked = linked_pairs.contains(&(m.left_provider.clone(), m.left_external_id.clone()));
    let right_linked =
        linked_pairs.contains(&(m.right_provider.clone(), m.right_external_id.clone()));
    if left_linked && !right_linked {
        true // use right
    } else {
        !right_linked || left_linked
    }
}

fn map_artist_suggestion(
    m: db::MatchSuggestion,
    linked_pairs: &HashSet<(String, String)>,
    link_lookup: &HashMap<(String, String), db::ArtistProviderLink>,
) -> yoink_shared::MatchSuggestion {
    let use_right = resolve_suggestion_side(&m, linked_pairs);

    let (provider, external_id, external_name, external_url, image_ref) = if use_right {
        (
            m.right_provider.clone(),
            m.right_external_id.clone(),
            m.external_name.clone(),
            m.external_url.clone(),
            m.image_ref.clone(),
        )
    } else if let Some(link) =
        link_lookup.get(&(m.left_provider.clone(), m.left_external_id.clone()))
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
}

fn map_album_suggestion(
    m: db::MatchSuggestion,
    linked_pairs: &HashSet<(String, String)>,
    link_lookup: &HashMap<(String, String), db::AlbumProviderLink>,
) -> yoink_shared::MatchSuggestion {
    let use_right = resolve_suggestion_side(&m, linked_pairs);

    let (provider, external_id, external_name, external_url, image_ref) = if use_right {
        (
            m.right_provider.clone(),
            m.right_external_id.clone(),
            m.external_name.clone(),
            m.external_url.clone(),
            m.image_ref.clone(),
        )
    } else if let Some(link) =
        link_lookup.get(&(m.left_provider.clone(), m.left_external_id.clone()))
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
}
