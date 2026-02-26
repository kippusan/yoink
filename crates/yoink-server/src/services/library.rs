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
/// Iterates provider links, fetches albums from each, deduplicates by title+release_date,
/// and creates/updates local album records + album provider links.
pub(crate) async fn sync_artist_albums(
    state: &AppState,
    artist_id: &str,
) -> Result<(), String> {
    let links = db::load_artist_provider_links(&state.db, artist_id)
        .await
        .map_err(|e| format!("failed to load provider links: {e}"))?;

    if links.is_empty() {
        return Err("No provider links found for this artist".to_string());
    }

    // Collect albums from all linked providers
    let mut all_incoming: Vec<(String, ProviderAlbum)> = Vec::new(); // (provider_id, album)

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

    // Deduplicate by title + release_date, preferring explicit, has-cover
    let mut deduped: HashMap<String, (String, ProviderAlbum)> = HashMap::new();
    for (provider_id, incoming) in all_incoming {
        let key = album_identity_key(&incoming.title, incoming.release_date.as_deref());
        deduped
            .entry(key)
            .and_modify(|(_, existing)| {
                if should_prefer_album(existing, &incoming) {
                    *existing = incoming.clone();
                }
            })
            .or_insert((provider_id, incoming));
    }

    // Build a map of identity_key -> external_id for the selected albums
    let selected_external_ids: HashMap<String, String> = deduped
        .iter()
        .map(|(key, (_, album))| (key.clone(), album.external_id.clone()))
        .collect();

    let mut albums = state.monitored_albums.write().await;

    // Process each incoming album
    for (provider_id, incoming) in deduped.into_values() {
        let ext_id_str = incoming.external_id.clone();

        // Check if we already have a local album linked to this provider ID
        let existing_album_id =
            db::find_album_by_provider_link(&state.db, &provider_id, &ext_id_str)
                .await
                .ok()
                .flatten();

        if let Some(local_album_id) = existing_album_id {
            // Update the existing local album
            if let Some(existing) = albums.iter_mut().find(|a| a.id == local_album_id) {
                existing.title = incoming.title;
                existing.album_type = incoming.album_type;
                existing.release_date = incoming.release_date;
                existing.cover_url = incoming.cover_ref.as_deref().map(|c| {
                    yoink_shared::provider_image_url(&provider_id, c, 640)
                });
                existing.explicit = incoming.explicit;
                let _ = db::upsert_album(&state.db, existing).await;
            }
        } else {
            // Create a new local album with a UUID
            let new_album_id = db::uuid_to_string(&db::new_uuid());
            let album = MonitoredAlbum {
                id: new_album_id.clone(),
                artist_id: artist_id.to_string(),
                title: incoming.title.clone(),
                album_type: incoming.album_type,
                release_date: incoming.release_date,
                cover_url: incoming.cover_ref.as_deref().map(|c| {
                    yoink_shared::provider_image_url(&provider_id, c, 640)
                }),
                explicit: incoming.explicit,
                monitored: false,
                acquired: false,
                wanted: false,
                added_at: Utc::now(),
            };
            let _ = db::upsert_album(&state.db, &album).await;

            // Create the provider link for this album
            let link = db::AlbumProviderLink {
                id: db::uuid_to_string(&db::new_uuid()),
                album_id: new_album_id.clone(),
                provider: provider_id.clone(),
                external_id: ext_id_str.clone(),
                external_url: incoming.url,
                external_title: Some(incoming.title),
                cover_ref: incoming.cover_ref,
            };
            let _ = db::upsert_album_provider_link(&state.db, &link).await;

            albums.push(album);
        }
    }

    // Remove deduped-out albums for this artist
    let artist_album_ids: Vec<String> = albums
        .iter()
        .filter(|a| a.artist_id == artist_id)
        .map(|a| a.id.clone())
        .collect();

    let mut ids_to_remove = Vec::new();
    for album_id in &artist_album_ids {
        let album_links = db::load_album_provider_links(&state.db, album_id)
            .await
            .unwrap_or_default();

        for album_link in &album_links {
            let album = albums.iter().find(|a| a.id == *album_id);
            if let Some(album) = album {
                let key = album_identity_key(&album.title, album.release_date.as_deref());
                if let Some(selected_ext_id) = selected_external_ids.get(&key) {
                    if album_link.external_id != *selected_ext_id {
                        ids_to_remove.push(album_id.clone());
                    }
                }
            }
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
    let artist_names: HashMap<String, String> =
        artists.into_iter().map(|a| (a.id.clone(), a.name)).collect();
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

        if import_local_album(state, &artist_id, &local.album_title, local.year.as_deref()).await?
        {
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
        if best_match.is_none() {
            if let Some(first) = candidates.first().cloned() {
                best_match = Some((provider_id.clone(), first));
            }
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
    format!(
        "{}|{}",
        title.trim().to_ascii_lowercase(),
        release_date.unwrap_or("")
    )
}

fn should_prefer_album(existing: &ProviderAlbum, candidate: &ProviderAlbum) -> bool {
    let existing_cover = existing.cover_ref.is_some();
    let candidate_cover = candidate.cover_ref.is_some();
    if candidate_cover != existing_cover {
        return candidate_cover;
    }

    let existing_explicit = existing.explicit;
    let candidate_explicit = candidate.explicit;
    if candidate_explicit != existing_explicit {
        return candidate_explicit;
    }

    candidate.external_id > existing.external_id
}
