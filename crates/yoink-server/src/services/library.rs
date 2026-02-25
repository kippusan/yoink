use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tokio::fs;
use tracing::info;

use crate::{
    db,
    models::{HifiAlbum, HifiArtistAlbumsResponse, MonitoredAlbum, MonitoredArtist},
    services::downloads::sanitize_path_component,
    state::AppState,
};

use super::hifi::{hifi_get_json, search_hifi_artists};

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

pub(crate) async fn sync_artist_albums_from_hifi(
    state: &AppState,
    artist_id: i64,
) -> Result<(), String> {
    let response = hifi_get_json::<HifiArtistAlbumsResponse>(
        state,
        "/artist/",
        vec![
            ("f".to_string(), artist_id.to_string()),
            ("skip_tracks".to_string(), "true".to_string()),
        ],
    )
    .await?;

    let mut deduped: HashMap<String, HifiAlbum> = HashMap::new();
    for incoming in response.albums.items {
        let key = album_identity_key(&incoming.title, incoming.release_date.as_deref());
        deduped
            .entry(key)
            .and_modify(|existing| {
                if should_prefer_album(existing, &incoming) {
                    *existing = incoming.clone();
                }
            })
            .or_insert(incoming);
    }

    let selected_ids_by_key = deduped
        .iter()
        .map(|(key, album)| (key.clone(), album.id))
        .collect::<HashMap<_, _>>();

    let mut albums = state.monitored_albums.write().await;

    for incoming in deduped.into_values() {
        if let Some(existing) = albums.iter_mut().find(|album| album.id == incoming.id) {
            existing.artist_id = artist_id;
            existing.title = incoming.title;
            existing.album_type = incoming.album_type;
            existing.release_date = incoming.release_date;
            existing.cover = incoming.cover;
            existing.tidal_url = incoming.url;
            existing.explicit = incoming.explicit.unwrap_or(false);
            // Persist updated album
            let _ = db::upsert_album(&state.db, existing).await;
        } else {
            let album = MonitoredAlbum {
                id: incoming.id,
                artist_id,
                title: incoming.title,
                album_type: incoming.album_type,
                release_date: incoming.release_date,
                cover: incoming.cover,
                tidal_url: incoming.url,
                explicit: incoming.explicit.unwrap_or(false),
                monitored: false,
                acquired: false,
                wanted: false,
                added_at: Utc::now(),
            };
            // Persist new album
            let _ = db::upsert_album(&state.db, &album).await;
            albums.push(album);
        }
    }

    // Remove deduped-out albums — collect IDs to delete first
    let removed_ids: Vec<i64> = albums
        .iter()
        .filter(|album| {
            if album.artist_id != artist_id {
                return false;
            }
            let key = album_identity_key(&album.title, album.release_date.as_deref());
            match selected_ids_by_key.get(&key) {
                Some(selected_id) => album.id != *selected_id,
                None => false,
            }
        })
        .map(|album| album.id)
        .collect();

    for id in &removed_ids {
        let _ = db::delete_album(&state.db, *id).await;
    }

    albums.retain(|album| {
        if album.artist_id != artist_id {
            return true;
        }

        let key = album_identity_key(&album.title, album.release_date.as_deref());
        match selected_ids_by_key.get(&key) {
            Some(selected_id) => album.id == *selected_id,
            None => true,
        }
    });

    Ok(())
}

pub(crate) async fn reconcile_library_files(state: &AppState) -> Result<usize, String> {
    let artists = state.monitored_artists.read().await.clone();
    let artist_names: HashMap<i64, String> = artists.into_iter().map(|a| (a.id, a.name)).collect();
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
            missing_ids.insert(album.id);
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
                album.id,
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
            && sync_artist_albums_from_hifi(state, artist_id).await.is_ok()
        {
            synced_artists.insert(artist_id);
        }

        if import_local_album(state, artist_id, &local.album_title, local.year.as_deref()).await? {
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

async fn ensure_monitored_artist(
    state: &AppState,
    artist_name: &str,
) -> Result<Option<(i64, bool)>, String> {
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

    let candidates = search_hifi_artists(state, artist_name).await?;
    let selected = candidates
        .iter()
        .find(|a| normalize_text(&a.name) == needle)
        .cloned()
        .or_else(|| candidates.into_iter().next());

    let Some(artist) = selected else {
        return Ok(None);
    };

    let monitored = MonitoredArtist {
        id: artist.id,
        name: artist.name,
        picture: artist.picture,
        tidal_url: artist.url,
        quality_profile: state.default_quality.clone(),
        added_at: Utc::now(),
    };
    let _ = db::upsert_artist(&state.db, &monitored).await;
    {
        let mut artists = state.monitored_artists.write().await;
        if artists.iter().all(|a| a.id != monitored.id) {
            artists.push(monitored.clone());
        }
    }

    Ok(Some((monitored.id, true)))
}

async fn import_local_album(
    state: &AppState,
    artist_id: i64,
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
            album.id,
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

fn should_prefer_album(existing: &HifiAlbum, candidate: &HifiAlbum) -> bool {
    let existing_cover = existing.cover.is_some();
    let candidate_cover = candidate.cover.is_some();
    if candidate_cover != existing_cover {
        return candidate_cover;
    }

    let existing_explicit = existing.explicit.unwrap_or(false);
    let candidate_explicit = candidate.explicit.unwrap_or(false);
    if candidate_explicit != existing_explicit {
        return candidate_explicit;
    }

    candidate.id > existing.id
}
