use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tokio::fs;
use tracing::info;

use crate::{
    db,
    models::{MonitoredAlbum, MonitoredArtist},
    providers::ProviderAlbum,
    services::downloads::sanitize_path_component,
    state::AppState,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScanImportSummary {
    pub(crate) discovered_albums: usize,
    pub(crate) imported_albums: usize,
    pub(crate) artists_added: usize,
    pub(crate) unmatched_albums: usize,
}

pub(crate) fn update_wanted(album: &mut MonitoredAlbum) {
    album.wanted = album.monitored && !album.acquired;
}

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
    // Each group collects ALL (provider_id, ProviderAlbum) entries that
    // share the same identity key, instead of picking a single winner.
    let mut groups: HashMap<String, Vec<(String, ProviderAlbum)>> = HashMap::new();
    for (provider_id, album) in all_incoming {
        let key = album_identity_key(&album.title, album.release_date.as_deref());
        groups.entry(key).or_default().push((provider_id, album));
    }

    // Merge dateless groups into dated ones with the same normalized title.
    // e.g. "albumtitle|" merges into "albumtitle|2024" if it exists.
    let dateless_keys: Vec<String> = groups
        .keys()
        .filter(|k| k.ends_with('|'))
        .cloned()
        .collect();
    for dateless_key in dateless_keys {
        let title_part = &dateless_key[..dateless_key.len() - 1]; // strip trailing '|'
        // Find a dated group with the same title prefix.
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
        // Pick the best album for display metadata.
        let (best_provider, best_album) =
            entries.iter().skip(1).fold(&entries[0], |acc, candidate| {
                if should_prefer_album(&acc.0, &acc.1, &candidate.0, &candidate.1) {
                    candidate
                } else {
                    acc
                }
            });

        // Try to find an existing local album via ANY of the provider links
        // in this group.
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
            // Update the existing local album with the best provider's metadata.
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
            // Create a new local album.
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

        // Upsert ALL provider links for this group, pointing at the same
        // local album. This ensures every provider's external ID is linked.
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
    // An album is stale if its identity key no longer matches any incoming
    // group. This handles albums that were removed from all providers.
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

pub(crate) async fn reconcile_library_files(state: &AppState) -> Result<usize, String> {
    let artists = state.monitored_artists.read().await.clone();
    let artist_names: HashMap<String, String> = artists
        .into_iter()
        .map(|a| (a.id.clone(), a.name))
        .collect();
    let albums_snapshot = state.monitored_albums.read().await.clone();

    let mut missing_ids = HashSet::new();
    for album in albums_snapshot.iter().filter(|a| a.acquired) {
        let Some(artist_name) = artist_names.get(&album.artist_id) else {
            continue;
        };
        let release_suffix = album
            .release_date
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        let album_dir = state
            .music_root
            .join(sanitize_path_component(artist_name))
            .join(sanitize_path_component(&format!(
                "{} ({})",
                album.title, release_suffix
            )));

        if !album_dir_has_downloaded_audio(&album_dir).await {
            missing_ids.insert(album.id.clone());
        }
    }

    if missing_ids.is_empty() {
        return Ok(0);
    }

    let mut changed = 0usize;
    let mut albums = state.monitored_albums.write().await;
    for album in albums.iter_mut() {
        if missing_ids.contains(&album.id) && album.acquired {
            album.acquired = false;
            update_wanted(album);
            let _ = db::update_album_flags(
                &state.db,
                &album.id,
                album.monitored,
                album.acquired,
                album.wanted,
            )
            .await;
            changed += 1;
        }
    }

    if changed > 0 {
        info!(
            updated_albums = changed,
            "Reconciled missing files in library"
        );
        state.notify_sse();
    }

    Ok(changed)
}

pub(crate) async fn merge_albums(
    state: &AppState,
    target_album_id: &str,
    source_album_id: &str,
) -> Result<(), String> {
    if target_album_id == source_album_id {
        return Err("target and source albums must be different".to_string());
    }

    let (target_artist_id, source_artist_id, source_flags) = {
        let albums = state.monitored_albums.read().await;
        let Some(target) = albums.iter().find(|a| a.id == target_album_id) else {
            return Err("target album not found".to_string());
        };
        let Some(source) = albums.iter().find(|a| a.id == source_album_id) else {
            return Err("source album not found".to_string());
        };
        (
            target.artist_id.clone(),
            source.artist_id.clone(),
            (source.monitored, source.acquired, source.wanted),
        )
    };

    if target_artist_id != source_artist_id {
        return Err("can only merge albums from same artist".to_string());
    }

    let source_links = db::load_album_provider_links(&state.db, source_album_id)
        .await
        .map_err(|e| format!("failed loading source provider links: {e}"))?;

    for link in source_links {
        let moved = db::AlbumProviderLink {
            id: db::uuid_to_string(&db::new_uuid()),
            album_id: target_album_id.to_string(),
            provider: link.provider,
            external_id: link.external_id,
            external_url: link.external_url,
            external_title: link.external_title,
            cover_ref: link.cover_ref,
        };
        let _ = db::upsert_album_provider_link(&state.db, &moved).await;
    }

    let _ = db::reassign_tracks_to_album(&state.db, source_album_id, target_album_id)
        .await
        .map_err(|e| format!("failed reassigning tracks: {e}"))?;
    let _ = db::reassign_jobs_to_album(&state.db, source_album_id, target_album_id)
        .await
        .map_err(|e| format!("failed reassigning jobs: {e}"))?;

    {
        let mut albums = state.monitored_albums.write().await;
        if let Some(target) = albums.iter_mut().find(|a| a.id == target_album_id) {
            target.monitored = target.monitored || source_flags.0;
            target.acquired = target.acquired || source_flags.1;
            update_wanted(target);
            let _ = db::update_album_flags(
                &state.db,
                &target.id,
                target.monitored,
                target.acquired,
                target.wanted,
            )
            .await;
        }
        albums.retain(|a| a.id != source_album_id);
    }

    db::delete_album(&state.db, source_album_id)
        .await
        .map_err(|e| format!("failed deleting source album: {e}"))?;

    Ok(())
}

pub(crate) async fn scan_and_import_library(state: &AppState) -> Result<ScanImportSummary, String> {
    let discovered = discover_local_albums(state).await?;
    if discovered.is_empty() {
        return Ok(ScanImportSummary {
            discovered_albums: 0,
            imported_albums: 0,
            artists_added: 0,
            unmatched_albums: 0,
        });
    }

    let mut imported_albums = 0usize;
    let mut artists_added = 0usize;
    let mut unmatched_albums = 0usize;
    let mut synced_artists = HashSet::new();

    for local in &discovered {
        let (artist_id, added_artist) = match ensure_monitored_artist(state, &local.artist_name)
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) => {
                unmatched_albums += 1;
                continue;
            }
            Err(err) => {
                info!(artist = %local.artist_name, error = %err, "Failed to resolve artist during import");
                unmatched_albums += 1;
                continue;
            }
        };

        if added_artist {
            artists_added += 1;
        }

        if !synced_artists.contains(&artist_id)
            && sync_artist_albums(state, &artist_id).await.is_ok()
        {
            synced_artists.insert(artist_id.clone());
        }

        if import_local_album(state, &artist_id, &local.album_title, local.year.as_deref()).await? {
            imported_albums += 1;
        } else {
            unmatched_albums += 1;
        }
    }

    if imported_albums > 0 {
        state.notify_sse();
    }

    Ok(ScanImportSummary {
        discovered_albums: discovered.len(),
        imported_albums,
        artists_added,
        unmatched_albums,
    })
}

#[derive(Debug, Clone)]
struct LocalAlbumDir {
    artist_name: String,
    album_title: String,
    year: Option<String>,
}

async fn discover_local_albums(state: &AppState) -> Result<Vec<LocalAlbumDir>, String> {
    if !fs::try_exists(&state.music_root).await.unwrap_or(false) {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let mut artist_dirs = fs::read_dir(&state.music_root).await.map_err(|err| {
        format!(
            "failed to read music root {}: {err}",
            state.music_root.display()
        )
    })?;

    while let Some(artist_entry) = artist_dirs
        .next_entry()
        .await
        .map_err(|err| format!("failed to read music root entry: {err}"))?
    {
        let artist_path = artist_entry.path();
        if !artist_path.is_dir() {
            continue;
        }
        let Some(artist_name) = artist_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        let mut album_dirs = match fs::read_dir(&artist_path).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        while let Some(album_entry) = album_dirs.next_entry().await.unwrap_or(None) {
            let album_path = album_entry.path();
            if !album_path.is_dir() || !album_dir_has_downloaded_audio(&album_path).await {
                continue;
            }
            let Some(album_folder) = album_path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let (album_title, year) = split_album_folder_name(album_folder);
            out.push(LocalAlbumDir {
                artist_name: artist_name.to_string(),
                album_title,
                year,
            });
        }
    }

    Ok(out)
}

/// Ensure an artist is monitored. Searches all metadata providers to find a match.
async fn ensure_monitored_artist(
    state: &AppState,
    artist_name: &str,
) -> Result<Option<(String, bool)>, String> {
    let needle = normalize_text(artist_name);
    {
        let artists = state.monitored_artists.read().await;
        if let Some(artist) = artists
            .iter()
            .find(|a| normalize_text(&a.name) == needle)
            .cloned()
        {
            return Ok(Some((artist.id, false)));
        }
    }

    // Search using all metadata providers
    let all_results = state.registry.search_artists_all(artist_name).await;

    let mut best_match: Option<(String, crate::providers::ProviderArtist)> = None;
    for (provider_id, candidates) in &all_results {
        // Prefer exact name match
        if let Some(exact) = candidates
            .iter()
            .find(|a| normalize_text(&a.name) == needle)
            .cloned()
        {
            best_match = Some((provider_id.clone(), exact));
            break;
        }
        // Otherwise take the first result from any provider
        if best_match.is_none()
            && let Some(first) = candidates.first().cloned()
        {
            best_match = Some((provider_id.clone(), first));
        }
    }

    let Some((provider_id, artist)) = best_match else {
        return Ok(None);
    };

    let new_id = db::uuid_to_string(&db::new_uuid());
    let image_url = artist
        .image_ref
        .as_deref()
        .map(|r| yoink_shared::provider_image_url(&provider_id, r, 640));

    let monitored = MonitoredArtist {
        id: new_id.clone(),
        name: artist.name.clone(),
        image_url,
        added_at: Utc::now(),
    };
    let _ = db::upsert_artist(&state.db, &monitored).await;

    // Create the provider link
    let link = db::ArtistProviderLink {
        id: db::uuid_to_string(&db::new_uuid()),
        artist_id: new_id.clone(),
        provider: provider_id,
        external_id: artist.external_id,
        external_url: artist.url,
        external_name: Some(artist.name),
        image_ref: artist.image_ref,
    };
    let _ = db::upsert_artist_provider_link(&state.db, &link).await;

    {
        let mut artists = state.monitored_artists.write().await;
        if artists.iter().all(|a| a.id != monitored.id) {
            artists.push(monitored.clone());
        }
    }

    Ok(Some((new_id, true)))
}

async fn import_local_album(
    state: &AppState,
    artist_id: &str,
    album_title: &str,
    year_hint: Option<&str>,
) -> Result<bool, String> {
    let target_title = normalize_text(album_title);
    let mut albums = state.monitored_albums.write().await;

    let mut matched_index = None;
    for (idx, album) in albums.iter().enumerate() {
        if album.artist_id != artist_id {
            continue;
        }
        if normalize_text(&album.title) != target_title {
            continue;
        }
        if let Some(year) = year_hint {
            let album_year = album.release_date.as_deref().and_then(parse_release_year);
            if album_year != Some(year.to_string()) {
                continue;
            }
        }
        matched_index = Some(idx);
        break;
    }

    let Some(idx) = matched_index else {
        return Ok(false);
    };

    let album = &mut albums[idx];
    let mut changed = false;
    if !album.monitored {
        album.monitored = true;
        changed = true;
    }
    if !album.acquired {
        album.acquired = true;
        changed = true;
    }
    update_wanted(album);
    if changed {
        let _ = db::update_album_flags(
            &state.db,
            &album.id,
            album.monitored,
            album.acquired,
            album.wanted,
        )
        .await;
    }
    Ok(changed)
}

async fn album_dir_has_downloaded_audio(path: &std::path::Path) -> bool {
    if !fs::try_exists(path).await.unwrap_or(false) {
        return false;
    }

    let mut entries = match fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let p = entry.path();
        if p.extension()
            .and_then(|e| e.to_str())
            .map(|ext| {
                ext.eq_ignore_ascii_case("flac")
                    || ext.eq_ignore_ascii_case("m4a")
                    || ext.eq_ignore_ascii_case("mp4")
            })
            .unwrap_or(false)
        {
            return true;
        }
    }

    false
}

fn split_album_folder_name(name: &str) -> (String, Option<String>) {
    if let Some((title, tail)) = name.rsplit_once(" (") {
        let year = tail.trim_end_matches(')').trim();
        if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
            return (title.trim().to_string(), Some(year.to_string()));
        }
    }
    (name.trim().to_string(), None)
}

fn parse_release_year(release_date: &str) -> Option<String> {
    let year = release_date.chars().take(4).collect::<String>();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        Some(year)
    } else {
        None
    }
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn album_identity_key(title: &str, release_date: Option<&str>) -> String {
    // Normalize date to year-only so "2023-06-16" and "2023" both produce "2023".
    // This prevents duplicates when providers return different date granularities.
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
            // Curly quotes → ASCII
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            // Dashes → ASCII hyphen
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' => '-',
            // Ellipsis
            '\u{2026}' => '.',
            other => other,
        })
        .flat_map(|c| c.to_lowercase())
        .collect();

    strip_featuring(&normalized)
}

/// Strip parenthesized or bracketed featuring clauses from a lowercased title.
/// Handles: (feat. …), (ft. …), (featuring …), [feat. …], [ft. …], [featuring …)
fn strip_featuring(title: &str) -> String {
    const FEAT_PREFIXES: &[&str] = &["feat. ", "feat ", "ft. ", "ft ", "featuring "];

    let mut result = title.to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        if let Some(start) = result.find(open) {
            let inner = &result[start + open.len_utf8()..];
            if let Some(end_offset) = inner.find(close) {
                let inner_trimmed = inner[..end_offset].trim_start();
                if FEAT_PREFIXES.iter().any(|p| inner_trimmed.starts_with(p)) {
                    // Remove the entire "(feat. …)" / "[feat. …]" clause
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
/// source for a merged album.  Priority order:
///   1. Has cover art  (any cover > no cover)
///   2. Provider priority  (download-capable / fast-image providers first)
///   3. Explicit flag  (explicit > clean, for more complete metadata)
///   4. External ID string ordering  (stable tiebreaker)
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

    // Prefer providers with faster image serving / download capability.
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
/// Download-capable providers with fast image APIs rank highest.
fn provider_priority(provider_id: &str) -> u8 {
    match provider_id {
        "tidal" => 10,
        "deezer" => 9,
        "musicbrainz" => 1,
        _ => 5,
    }
}
