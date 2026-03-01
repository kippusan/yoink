use std::collections::{HashMap, HashSet};

use chrono::Utc;
use uuid::Uuid;

use crate::{
    db,
    models::MonitoredAlbum,
    providers::{ProviderAlbum, ProviderArtist, ProviderTrack},
    state::AppState,
};

pub(crate) async fn recompute_artist_match_suggestions(
    state: &AppState,
    artist_id: Uuid,
) -> Result<(), String> {
    let artist_name = {
        let artists = state.monitored_artists.read().await;
        artists
            .iter()
            .find(|a| a.id == artist_id)
            .map(|a| a.name.clone())
            .ok_or_else(|| format!("artist {artist_id} not found"))?
    };

    let albums: Vec<MonitoredAlbum> = {
        let albums = state.monitored_albums.read().await;
        albums
            .iter()
            .filter(|a| a.artist_id == artist_id)
            .cloned()
            .collect()
    };

    let artist_links = db::load_artist_provider_links(&state.db, artist_id)
        .await
        .map_err(|e| format!("failed to load artist provider links: {e}"))?;

    let _ = db::clear_pending_match_suggestions(&state.db, "artist", artist_id).await;

    recompute_artist_level_suggestions(state, artist_id, &artist_name, &artist_links).await?;

    for album in albums {
        recompute_album_match_suggestions(state, &album, &artist_name).await?;
    }

    Ok(())
}

async fn recompute_artist_level_suggestions(
    state: &AppState,
    artist_id: Uuid,
    artist_name: &str,
    artist_links: &[db::ArtistProviderLink],
) -> Result<(), String> {
    if artist_links.is_empty() {
        return Ok(());
    }

    let existing_pairs: HashSet<(String, String)> = artist_links
        .iter()
        .map(|l| (l.provider.clone(), l.external_id.clone()))
        .collect();

    let reference = artist_links
        .iter()
        .max_by_key(|l| provider_priority(&l.provider))
        .unwrap_or(&artist_links[0]);

    for provider_id in state.registry.metadata_provider_ids() {
        let Some(provider) = state.registry.metadata_provider(&provider_id) else {
            continue;
        };

        let results = provider
            .search_artists(artist_name)
            .await
            .unwrap_or_default();
        let Some((candidate, score)) = best_artist_candidate(artist_name, results, |candidate| {
            !existing_pairs.contains(&(provider_id.clone(), candidate.external_id.clone()))
        }) else {
            continue;
        };

        let confidence = (score * 100.0).round().clamp(0.0, 100.0) as u8;
        if confidence < 86 {
            continue;
        }

        let now = Utc::now();
        let suggestion = db::MatchSuggestion {
            id: Uuid::now_v7(),
            scope_type: "artist".to_string(),
            scope_id: artist_id,
            left_provider: reference.provider.clone(),
            left_external_id: reference.external_id.clone(),
            right_provider: provider_id.clone(),
            right_external_id: candidate.external_id.clone(),
            match_kind: "fuzzy".to_string(),
            confidence,
            explanation: Some(format!("Artist name fuzzy {:.0}%", score * 100.0)),
            external_name: Some(candidate.name),
            external_url: candidate.url,
            image_ref: candidate.image_ref,
            disambiguation: candidate.disambiguation,
            artist_type: candidate.artist_type,
            country: candidate.country,
            tags: candidate.tags,
            popularity: candidate.popularity,
            status: "pending".to_string(),
            created_at: now,
            updated_at: now,
        };

        let _ = db::upsert_match_suggestion(&state.db, &suggestion).await;
    }

    Ok(())
}

async fn recompute_album_match_suggestions(
    state: &AppState,
    album: &MonitoredAlbum,
    artist_name: &str,
) -> Result<(), String> {
    let existing_links = db::load_album_provider_links(&state.db, album.id)
        .await
        .map_err(|e| format!("failed to load album provider links: {e}"))?;

    let _ = db::clear_pending_match_suggestions(&state.db, "album", album.id).await;

    if existing_links.is_empty() {
        return Ok(());
    }

    let metadata_links: Vec<_> = existing_links
        .iter()
        .filter(|l| state.registry.metadata_provider(&l.provider).is_some())
        .collect();

    if metadata_links.is_empty() {
        return Ok(());
    }

    let mut link_by_provider: HashMap<String, (String, String)> = HashMap::new();
    for l in &metadata_links {
        link_by_provider.insert(
            l.provider.clone(),
            (l.provider.clone(), l.external_id.clone()),
        );
    }

    let existing_pairs: HashSet<(String, String)> = metadata_links
        .iter()
        .map(|l| (l.provider.clone(), l.external_id.clone()))
        .collect();

    let (reference_provider, reference_tracks) = pick_reference_tracks(state, &metadata_links)
        .await
        .unwrap_or_else(|| (metadata_links[0].provider.clone(), Vec::new()));

    let reference_pair = link_by_provider
        .get(&reference_provider)
        .cloned()
        .unwrap_or_else(|| {
            (
                metadata_links[0].provider.clone(),
                metadata_links[0].external_id.clone(),
            )
        });

    for provider_id in state.registry.metadata_provider_ids() {
        let Some(provider) = state.registry.metadata_provider(&provider_id) else {
            continue;
        };

        let artists = provider
            .search_artists(artist_name)
            .await
            .unwrap_or_default();

        let candidates = best_album_candidates(provider.as_ref(), artists, album).await;
        let Some((candidate_album, album_score)) = candidates else {
            continue;
        };

        if existing_pairs.contains(&(provider_id.clone(), candidate_album.external_id.clone())) {
            continue;
        }

        let target_tracks = provider
            .fetch_tracks(&candidate_album.external_id)
            .await
            .map(|(tracks, _)| tracks)
            .unwrap_or_default();

        let isrc_overlap = count_isrc_overlap(&reference_tracks, &target_tracks);
        let title_overlap = track_title_overlap(&reference_tracks, &target_tracks);

        let (match_kind, confidence, explanation) = if isrc_overlap > 0 {
            let confidence = (90 + (isrc_overlap as u8).saturating_mul(3)).min(100);
            (
                "isrc_exact".to_string(),
                confidence,
                Some(format!("ISRC overlap: {isrc_overlap} track(s)")),
            )
        } else {
            let combined = ((album_score * 0.7) + (title_overlap * 0.3)) * 100.0;
            let confidence = combined.round().clamp(0.0, 100.0) as u8;
            if confidence < 82 {
                continue;
            }
            (
                "fuzzy".to_string(),
                confidence,
                Some(format!(
                    "Fuzzy match album={:.0}% track={:.0}%",
                    album_score * 100.0,
                    title_overlap * 100.0
                )),
            )
        };

        let now = Utc::now();
        let suggestion = db::MatchSuggestion {
            id: Uuid::now_v7(),
            scope_type: "album".to_string(),
            scope_id: album.id,
            left_provider: reference_pair.0.clone(),
            left_external_id: reference_pair.1.clone(),
            right_provider: provider_id.clone(),
            right_external_id: candidate_album.external_id.clone(),
            match_kind,
            confidence,
            explanation,
            external_name: Some(candidate_album.title.clone()),
            external_url: candidate_album.url.clone(),
            image_ref: candidate_album.cover_ref.clone(),
            disambiguation: None,
            artist_type: None,
            country: None,
            tags: Vec::new(),
            popularity: None,
            status: "pending".to_string(),
            created_at: now,
            updated_at: now,
        };

        let _ = db::upsert_match_suggestion(&state.db, &suggestion).await;
    }

    Ok(())
}

async fn pick_reference_tracks(
    state: &AppState,
    links: &[&db::AlbumProviderLink],
) -> Option<(String, Vec<ProviderTrack>)> {
    let mut sorted: Vec<&db::AlbumProviderLink> = links.to_vec();
    sorted.sort_by_key(|l| std::cmp::Reverse(provider_priority(&l.provider)));

    for link in sorted {
        let provider = state.registry.metadata_provider(&link.provider)?;
        if let Ok((tracks, _)) = provider.fetch_tracks(&link.external_id).await
            && !tracks.is_empty()
        {
            return Some((link.provider.clone(), tracks));
        }
    }

    None
}

async fn best_album_candidates(
    provider: &dyn crate::providers::MetadataProvider,
    artists: Vec<ProviderArtist>,
    local_album: &MonitoredAlbum,
) -> Option<(ProviderAlbum, f64)> {
    let local_title = normalize(&local_album.title);
    let local_year = local_album
        .release_date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .map(|s| s.to_string());

    let mut best: Option<(ProviderAlbum, f64)> = None;

    for artist in artists.into_iter().take(4) {
        let albums = provider.fetch_albums(&artist.external_id).await.ok()?;
        for album in albums {
            let candidate_title = normalize(&album.title);
            let mut score = strsim::jaro_winkler(&local_title, &candidate_title);

            if let (Some(local_year), Some(candidate_date)) =
                (&local_year, album.release_date.as_deref())
                && candidate_date.starts_with(local_year)
            {
                score = (score + 0.08).min(1.0);
            }

            if best.as_ref().is_none_or(|(_, s)| score > *s) {
                best = Some((album, score));
            }
        }
    }

    best.filter(|(_, score)| *score >= 0.65)
}

fn count_isrc_overlap(left: &[ProviderTrack], right: &[ProviderTrack]) -> usize {
    let left_isrc: HashSet<String> = left
        .iter()
        .filter_map(|t| t.isrc.as_ref())
        .map(|v| v.trim().to_ascii_uppercase())
        .filter(|v| !v.is_empty())
        .collect();

    if left_isrc.is_empty() {
        return 0;
    }

    right
        .iter()
        .filter_map(|t| t.isrc.as_ref())
        .map(|v| v.trim().to_ascii_uppercase())
        .filter(|v| !v.is_empty())
        .filter(|isrc| left_isrc.contains(isrc))
        .count()
}

fn track_title_overlap(left: &[ProviderTrack], right: &[ProviderTrack]) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let left_titles: Vec<String> = left.iter().map(|t| normalize(&t.title)).collect();
    let right_titles: Vec<String> = right.iter().map(|t| normalize(&t.title)).collect();

    let mut total = 0.0;
    let mut matched = 0usize;
    for lt in &left_titles {
        let best = right_titles
            .iter()
            .map(|rt| strsim::jaro_winkler(lt, rt))
            .fold(0.0, f64::max);
        total += best;
        matched += 1;
    }

    if matched == 0 {
        0.0
    } else {
        total / matched as f64
    }
}

fn normalize(input: &str) -> String {
    input
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn provider_priority(provider_id: &str) -> u8 {
    match provider_id {
        "tidal" => 10,
        "deezer" => 9,
        "musicbrainz" => 1,
        _ => 5,
    }
}

fn best_artist_candidate(
    local_artist_name: &str,
    candidates: Vec<ProviderArtist>,
    mut predicate: impl FnMut(&ProviderArtist) -> bool,
) -> Option<(ProviderArtist, f64)> {
    let local = normalize(local_artist_name);
    candidates
        .into_iter()
        .filter(|candidate| predicate(candidate))
        .map(|candidate| {
            let score = strsim::jaro_winkler(&local, &normalize(&candidate.name));
            (candidate, score)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .and_then(|best| if best.1 >= 0.70 { Some(best) } else { None })
}
