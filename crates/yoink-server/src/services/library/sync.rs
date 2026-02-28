use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tracing::info;

use crate::{db, models::MonitoredAlbum, providers::ProviderAlbum, state::AppState};

/// Sync albums for an artist from all linked metadata providers.
/// Groups incoming albums by identity key (title + year), merges all provider
/// links onto a single local album per key, and picks the best provider source
/// for display metadata.
pub(crate) async fn sync_artist_albums(state: &AppState, artist_id: &str) -> Result<(), String> {
    let links = db::load_artist_provider_links(&state.db, artist_id)
        .await
        .map_err(|e| format!("failed to load provider links: {e}"))?;

    if links.is_empty() {
        return Err("No provider links found for this artist".to_string());
    }

    // ── 1. Collect albums from all linked providers ──────────────────────
    let mut all_incoming: Vec<(String, ProviderAlbum)> = Vec::new();

    for link in &links {
        let Some(provider) = state.registry.metadata_provider(&link.provider) else {
            continue;
        };

        match provider.fetch_albums(&link.external_id).await {
            Ok(albums) => {
                for album in albums {
                    all_incoming.push((link.provider.clone(), album));
                }
            }
            Err(err) => {
                info!(
                    provider = %link.provider,
                    artist_id = artist_id,
                    error = %err.0,
                    "Failed to fetch albums from provider"
                );
            }
        }
    }

    if all_incoming.is_empty() {
        return Ok(());
    }

    // ── 2. Group by identity key (title + year) ─────────────────────────
    let mut groups: HashMap<String, Vec<(String, ProviderAlbum)>> = HashMap::new();
    for (provider_id, album) in all_incoming {
        let key = album_identity_key(&album.title, album.release_date.as_deref());
        groups.entry(key).or_default().push((provider_id, album));
    }

    // Merge dateless groups into dated ones with the same normalized title.
    let dateless_keys: Vec<String> = groups
        .keys()
        .filter(|k| k.ends_with('|'))
        .cloned()
        .collect();
    for dateless_key in dateless_keys {
        let title_part = &dateless_key[..dateless_key.len() - 1];
        let dated_match = groups
            .keys()
            .find(|k| {
                *k != &dateless_key
                    && k.starts_with(title_part)
                    && k.as_bytes().get(title_part.len()) == Some(&b'|')
            })
            .cloned();
        if let Some(dated_key) = dated_match
            && let Some(entries) = groups.remove(&dateless_key)
        {
            groups.entry(dated_key).or_default().extend(entries);
        }
    }

    // Track which identity keys were seen so we can clean up stale albums.
    let incoming_keys: HashSet<String> = groups.keys().cloned().collect();

    let mut albums = state.monitored_albums.write().await;

    // ── 3. Process each group ───────────────────────────────────────────
    for entries in groups.values() {
        let (best_provider, best_album) =
            entries.iter().skip(1).fold(&entries[0], |acc, candidate| {
                if should_prefer_album(&acc.0, &acc.1, &candidate.0, &candidate.1) {
                    candidate
                } else {
                    acc
                }
            });

        // Try to find an existing local album via ANY of the provider links.
        let mut local_album_id: Option<String> = None;
        for (prov, album) in entries {
            if let Ok(Some(id)) =
                db::find_album_by_provider_link(&state.db, prov, &album.external_id).await
            {
                local_album_id = Some(id);
                break;
            }
        }

        let album_id = if let Some(existing_id) = local_album_id {
            if let Some(existing) = albums.iter_mut().find(|a| a.id == existing_id) {
                existing.title = best_album.title.clone();
                existing.album_type = best_album.album_type.clone();
                existing.release_date = best_album.release_date.clone();
                existing.cover_url = best_album
                    .cover_ref
                    .as_deref()
                    .map(|c| yoink_shared::provider_image_url(best_provider, c, 640));
                existing.explicit = best_album.explicit;
                let _ = db::upsert_album(&state.db, existing).await;
            }
            existing_id
        } else {
            let new_id = db::uuid_to_string(&db::new_uuid());
            let album = MonitoredAlbum {
                id: new_id.clone(),
                artist_id: artist_id.to_string(),
                title: best_album.title.clone(),
                album_type: best_album.album_type.clone(),
                release_date: best_album.release_date.clone(),
                cover_url: best_album
                    .cover_ref
                    .as_deref()
                    .map(|c| yoink_shared::provider_image_url(best_provider, c, 640)),
                explicit: best_album.explicit,
                monitored: false,
                acquired: false,
                wanted: false,
                added_at: Utc::now(),
            };
            let _ = db::upsert_album(&state.db, &album).await;
            albums.push(album);
            new_id
        };

        // Upsert ALL provider links for this group.
        for (prov, album) in entries {
            let link = db::AlbumProviderLink {
                id: db::uuid_to_string(&db::new_uuid()),
                album_id: album_id.clone(),
                provider: prov.clone(),
                external_id: album.external_id.clone(),
                external_url: album.url.clone(),
                external_title: Some(album.title.clone()),
                cover_ref: album.cover_ref.clone(),
            };
            let _ = db::upsert_album_provider_link(&state.db, &link).await;
        }
    }

    // ── 4. Remove stale albums ──────────────────────────────────────────
    let mut ids_to_remove = Vec::new();
    for album in albums.iter().filter(|a| a.artist_id == artist_id) {
        let key = album_identity_key(&album.title, album.release_date.as_deref());
        if !incoming_keys.contains(&key) {
            ids_to_remove.push(album.id.clone());
        }
    }

    for id in &ids_to_remove {
        let _ = db::delete_album(&state.db, id).await;
    }
    albums.retain(|album| !ids_to_remove.contains(&album.id));

    Ok(())
}

fn album_identity_key(title: &str, release_date: Option<&str>) -> String {
    let year = release_date
        .and_then(|d| d.split('-').next())
        .filter(|y| !y.is_empty());
    format!("{}|{}", normalize_title(title), year.unwrap_or(""))
}

/// Normalize a title for deduplication: lowercase, collapse Unicode punctuation
/// to ASCII equivalents, and strip featuring suffixes so that
/// "First Time (feat. Elipsa)" and "First Time" produce the same key.
fn normalize_title(title: &str) -> String {
    let normalized: String = title
        .trim()
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' => '-',
            '\u{2026}' => '.',
            other => other,
        })
        .flat_map(|c| c.to_lowercase())
        .collect();

    strip_featuring(&normalized)
}

/// Strip parenthesized or bracketed featuring clauses from a lowercased title.
fn strip_featuring(title: &str) -> String {
    const FEAT_PREFIXES: &[&str] = &["feat. ", "feat ", "ft. ", "ft ", "featuring "];

    let mut result = title.to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let Some(start) = result.find(open) {
            let inner = &result[start + open.len_utf8()..];
            if let Some(end_offset) = inner.find(close) {
                let inner_trimmed = inner[..end_offset].trim_start();
                if FEAT_PREFIXES.iter().any(|p| inner_trimmed.starts_with(p)) {
                    let end = start + open.len_utf8() + end_offset + close.len_utf8();
                    result = format!("{}{}", &result[..start], &result[end..]);
                    result = result.trim().to_string();
                }
            }
        }
    }
    result
}

/// Decide whether `candidate` should replace `existing` as the display-metadata
/// source for a merged album.
fn should_prefer_album(
    existing_provider: &str,
    existing: &ProviderAlbum,
    candidate_provider: &str,
    candidate: &ProviderAlbum,
) -> bool {
    let existing_cover = existing.cover_ref.is_some();
    let candidate_cover = candidate.cover_ref.is_some();
    if candidate_cover != existing_cover {
        return candidate_cover;
    }

    let existing_prio = provider_priority(existing_provider);
    let candidate_prio = provider_priority(candidate_provider);
    if candidate_prio != existing_prio {
        return candidate_prio > existing_prio;
    }

    let existing_explicit = existing.explicit;
    let candidate_explicit = candidate.explicit;
    if candidate_explicit != existing_explicit {
        return candidate_explicit;
    }

    candidate.external_id > existing.external_id
}

/// Higher value = preferred as display-metadata source.
fn provider_priority(provider_id: &str) -> u8 {
    match provider_id {
        "tidal" => 10,
        "deezer" => 9,
        "musicbrainz" => 1,
        _ => 5,
    }
}
